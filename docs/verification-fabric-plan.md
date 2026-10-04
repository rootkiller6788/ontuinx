# OntoAssure Verification Fabric — 完整架构计划

**从 OpenCodeReview 中提取可普遍化的验证过程工程，吸收到 OntoAssure 体系**

---

## 目录

1. [架构总览](#1-架构总览)
2. [类型层 — onto-assurance-types 新增](#2-类型层--onto-assurance-types-新增)
3. [纯函数层 — onto-assurance-core 新增](#3-纯函数层--onto-assurance-core-新增)
4. [运行时层 — onto-assurance-runtime 新增](#4-运行时层--onto-assurance-runtime-新增)
5. [代码领域实现 — onto-code-pack](#5-代码领域实现--onto-code-pack)
6. [Go→Rust 算法移植指南](#6-go→rust-算法移植指南)
7. [VerifierProvider 接口](#7-verifierprovider-接口)
8. [线路协议 — 与 Go OntoFlow 的契约](#8-线路协议--与-go-ontoflow-的契约)
9. [IronClaw 适配 — 从全能代理到 SemanticVerifier](#9-ironclaw-适配--从全能代理到-semanticverifier)
10. [迁移路径](#10-迁移路径)

---

## 1. 架构总览

### 1.1 四层最终架构

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Layer 1: OntoFlow (Temporal Go)                                          │
│                                                                           │
│  职责: 多 WorkItem 编排、DAG、并发、持久化、重试、超时                      │
│  不负责: 验证逻辑、决策逻辑                                                 │
│                                                                           │
│  WorkItem → ActivityTask → Worker                                         │
└───────────────────────────────┬───────────────────────────────────────────┘
                                │ LoopInvocationRequest
                                ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  Layer 2: OntoLoop (Rust)                                                  │
│                                                                           │
│  职责: 单任务多Attempt循环、进展跟踪、预算控制、恢复                          │
│  不负责: Agent执行细节、验证细节                                            │
│                                                                           │
│  while (未完成 && 有预算) { attempt → evaluate → feedback }                 │
└───────────────────────────────┬───────────────────────────────────────────┘
                                │ start_run(objective)
                                ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  Layer 3: IronClaw / OntoRuntime (Rust)                                    │
│                                                                           │
│  职责: 一个Attempt的真实Agent执行 —— 使用工具、调用LLM、生成候选产物        │
│  双重角色:                                                                 │
│    Lane A — Agent Execution Lane: 完成开放式执行任务                        │
│    Lane B — Semantic Verification Lane: 被OntoAssure受限调用，产出         │
│              FindingCandidate[] (仅此，不产出Decision)                      │
│                                                                           │
│  不拥有: 范围选择、规则解释、覆盖证明、Evidence宣告、Success宣告            │
└────────────────────────────────┬──────────────────────────────────────────┘
                                 │ Candidate Checkpoint / FindingCandidate[]
                                 ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  Layer 4: OntoAssure (Rust)                                                │
│                                                                           │
│  ┌─────────────────────────────────────────────────────────────────┐     │
│  │  Verification Fabric (新增 — 吸收 OpenCodeReview 普遍化机制)      │     │
│  │                                                                   │     │
│  │  ScopeResolver ──▶ RuleRouter ──▶ Planner ──▶ Scheduler          │     │
│  │       │                │              │            │              │     │
│  │  ScopeManifest    RuleBinding    VerificationPlan  │              │     │
│  │                                            VerificationUnit      │     │
│  │                                                  │               │     │
│  │                          ┌───────────────────────┘               │     │
│  │                          ▼                                        │     │
│  │                    Verifier Scheduling                            │     │
│  │                          │                                        │     │
│  │          ┌───────────────┼───────────────┐                        │     │
│  │          ▼               ▼               ▼                        │     │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐                    │     │
│  │  │ IronClaw   │ │ Build/Test │ │ SAST/Policy│                    │     │
│  │  │ Semantic   │ │ /Lint      │ │ Verifier   │                    │     │
│  │  │ Verifier   │ │ Verifier   │ │            │                    │     │
│  │  └─────┬──────┘ └─────┬──────┘ └─────┬──────┘                    │     │
│  │        └──────────────┴──────────────┘                            │     │
│  │                       │                                           │     │
│  │              FindingCandidate[]                                    │     │
│  │                       │                                           │     │
│  │                       ▼                                           │     │
│  │  LocationResolver ──▶ Deduplicator ──▶ EvidenceBuilder           │     │
│  │                                                  │                │     │
│  │                                          EvidenceRecord            │     │
│  └──────────────────────────────────────────────────┼────────────────┘     │
│                                                     │                      │
│  ┌──────────────────────────────────────────────────┼────────────────┐     │
│  │  Evidence & Decision Authority (已有 M5/M6)       │                │     │
│  │                                                   ▼                │     │
│  │  Reducer ──▶ SessionDecision ──▶ AttemptDecision ──▶ Settlement    │     │
│  │                                                                   │     │
│  │  不变量: 只有 Decision Authority 可以宣告 TaskOutcome               │     │
│  │         只有 Settlement Authority 可以授权 Commit                   │     │
│  └───────────────────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 OpenCodeReview 能力归属

OpenCodeReview 被吸收的**不是**"代码审查能力"，而是**"如何把非确定性的 Verifier 装进确定性的验证流水线"**这一工程方法。

```
OpenCodeReview 机制                      →  OntoAssure 归属组件
══════════════════                      ══════════════════════

确定性文件选择 (FileFilter)              → ScopeResolver
全仓扫描 / Diff扫描 (Provider)           → TargetDiscovery
规则模板和路径路由 (Resolver)             → RuleRouter
文件分片和并行调度 (agent bundling)       → Planner / Scheduler
Token与时间预算 (--max-tokens-budget)    → VerificationBudget
Session持久化与恢复 (ResumeState)        → VerificationSession
行号修正和位置绑定 (ResolveLineNumbers)   → LocationResolver
评论过滤与去重 (ReviewFilterTask)         → FindingNormalizer
Finding→Evidence 升级                    → EvidenceBuilder
最终通过/失败/环境错误                     → DecisionAuthority (已有)
```

### 1.3 三者的本体职责

| 系统 | 本体职责 | 不应拥有的权力 |
|------|---------|--------------|
| **IronClaw** | 执行 Agent Loop、使用工具、生成候选产物、进行语义分析 | 选择最终验证范围、确认覆盖完整、生成权威 Evidence、宣布 Success |
| **OntoAssure** | 定义契约、组织验证、收集证据、归约 Criteria、作出 Decision、授权副作用 | 执行开放式任务、替代 Agent Runtime |
| **OpenCodeReview 机制** | 为验证过程提供确定性工程方法 | 独立成为第二套 Runtime 或第二套 Decision Authority |

最精确的说法：

> **IronClaw 是被验证的执行主体，同时也可以作为 OntoAssure 调用的一个语义 Verifier；OpenCodeReview 式机制属于调用和约束 Verifier 的控制面。**

### 1.4 IronClaw 的双重角色

```
IronClaw Runtime
├── Lane A: Agent Execution Lane
│   └── 被 OntoLoop 调用，完成一个 Attempt 中的开放式执行
│       输入: objective (自然语言任务描述)
│       输出: Candidate Checkpoint
│
└── Lane B: Semantic Verification Lane
    └── 被 OntoAssure Verification Fabric 受限调用
        输入: VerificationUnit + RuleBindings + BoundedContext + BudgetGrant
        输出: FindingCandidate[] (仅此！不产出 Decision、不宣告 Success)
```

关键：调用方向决定了控制权。

```
执行阶段 (Lane A):
  OntoLoop → IronClaw → Checkpoint

验证阶段 (Lane B):
  OntoAssure → Scope/Rules/Plan → 调度 IronClaw Lane B + 确定性 Verifier
             → FindingCandidate[] → Evidence → Decision
```

IronClaw 没有资格修改：ScopeManifest、RuleBinding、VerificationPlan、RequiredVerifier 集合、Criterion 映射、Evidence 强度、最终 Decision。

### 1.5 原则

1. **确定性优先** — 范围、规则路由、计划生成、位置解析全部是纯函数
2. **Verifier 只做语义分析** — 不给 Verifier 范围选择权、规则解释权、成功宣告权
3. **Finding 不是 Evidence** — 必须经过 LocationResolution + Dedup + Cross-verification → 才能成为 EvidenceRecord
4. **不做独立运行时** — 不保留 OpenCodeReview Go 进程，全部在 OntoAssure Rust 体系中重构
5. **Lane A 和 Lane B 是同一个 IronClaw 的两个调用模式** — 不是两个独立系统

---

## 2. 类型层 — onto-assurance-types 新增

### 2.1 文件: `src/verification_target.rs`

```rust
//! VerificationTarget — 通用验证目标，不限于代码。

use serde::{Deserialize, Serialize};
use crate::ids::VerificationTargetId;
use crate::hash::{HashDomain, ContentHash};

/// 一个需要被验证的独立目标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationTarget {
    pub target_id: VerificationTargetId,

    /// 目标种类 — 决定 Verifier 如何解释 target_ref
    pub target_kind: TargetKind,

    /// 目标引用 — 由 target_kind 决定含义
    ///   SourceFile  → repo-relative path
    ///   Document    → doc-path + section anchor
    ///   Dataset     → schema.table.column
    ///   Workflow    → namespace.workflow.node
    ///   Resource    → resource_type://resource_id
    pub target_ref: String,

    /// 目标的完整内容的哈希 — 用于事后验证"旧结论是否仍然适用"
    pub content_hash: ContentHash,

    /// 目标的字节大小（近似），用于预算估算
    pub size_bytes: u64,

    /// 编程语言 / 文档格式 / 数据格式
    pub language: Option<String>,

    /// 风险等级 — 从 contract 或 profile 派生
    pub risk_level: RiskLevel,

    /// 附加元数据 — 标签、所有者、模块名等
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    SourceFile,
    DiffHunk,
    Symbol,
    Document,
    Dataset,
    Workflow,
    ExternalResource,
    Configuration,
}

impl VerificationTarget {
    /// 计算该目标的指纹 — 若任一参数变化，旧结果失效
    pub fn fingerprint(&self, domain: &HashDomain) -> ContentHash {
        // hash(target_kind || target_ref || content_hash || language)
        todo!("见 Go→Rust 移植 §6.1")
    }
}
```

### 2.2 文件: `src/scope_manifest.rs`

```rust
//! ScopeManifest — 范围证明：每个应验证目标要么 Included 要么 Excluded(reason)

use serde::{Deserialize, Serialize};
use crate::ids::{VerificationTargetId, VerifierVersion};
use crate::hash::ContentHash;
use crate::enums::RiskLevel;

/// 被纳入验证范围的目标。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedTarget {
    pub target_id: VerificationTargetId,
    pub target_ref: String,
    pub target_kind: super::verification_target::TargetKind,
    pub risk_level: RiskLevel,
}

/// 被排除的目标及其原因——不能静默遗漏。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedTarget {
    pub target_ref: String,
    pub reason: ExclusionReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Glob 模式不匹配
    NotMatched,
    /// 超出预算
    BudgetExceeded,
    /// 被 FileFilter 排除
    FilteredOut,
    /// 二进制文件或不可审查
    NonReviewable,
    /// 被 contract/policy 显式排除
    PolicyExcluded,
    /// 与前一 Session 无变化
    Unchanged,
}

/// 范围清单 — 一次验证运行的完整目标列表。
///
/// 核心不变量:
///   S-1: included ∪ excluded = 所有发现的候选目标
///   S-2: included ∩ excluded = ∅
///   S-3: manifest_hash 覆盖所有字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeManifest {
    pub included_targets: Vec<ScopedTarget>,
    pub excluded_targets: Vec<ExcludedTarget>,
    pub resolver_version: VerifierVersion,
    pub manifest_hash: ContentHash,
}

impl ScopeManifest {
    /// 验证不变量 S-1 和 S-2（纯函数）
    pub fn validate_invariants(&self, all_candidate_refs: &[String]) -> Result<(), String> {
        // 每个候选目标必须出现在 included 或 excluded 中
        // included 和 excluded 不得有交集
        todo!("见 Go→Rust 移植 §6.2")
    }

    /// 从 included + excluded + metadata 重新计算哈希
    pub fn compute_manifest_hash(&self) -> ContentHash {
        todo!("见 Go→Rust 移植 §6.2")
    }
}
```

### 2.3 文件: `src/verification_plan.rs`

```rust
//! VerificationPlan — 将 ScopeManifest 分解为可并行执行的 VerificationUnit

use serde::{Deserialize, Serialize};
use crate::ids::{VerificationPlanId, VerificationUnitId};
use crate::hash::ContentHash;

/// 一次验证运行的计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub plan_id: VerificationPlanId,
    pub manifest_hash: ContentHash,
    pub units: Vec<VerificationUnit>,
    pub total_estimated_tokens: u64,
    pub max_parallel_units: u32,
}

/// 单个验证单元 — 最低调度粒度和恢复粒度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationUnit {
    pub unit_id: VerificationUnitId,

    /// 此单元要验证的目标引用
    pub target_refs: Vec<String>,

    /// 适用的规则绑定
    pub rule_bindings: Vec<RuleBindingRef>,

    /// 此单元所依赖的制品（如 test output、build result）
    pub dependency_refs: Vec<String>,

    /// 分配给此单元的预算
    pub budget_grant: VerificationBudgetGrant,

    /// 单元指纹 — 用于 resume: 若指纹不变，可跳过
    ///
    /// 指纹覆盖:
    ///   - 所有 target_ref 的 content_hash
    ///   - 所有 rule_binding 的 rule_pack_hash
    ///   - verifier 的 version_hash
    pub unit_fingerprint: ContentHash,
}

/// 对规则包的引用（不内联完整规则内容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleBindingRef {
    pub rule_pack_id: String,
    pub rule_pack_hash: ContentHash,
    pub verifier_id: super::ids::VerifierId,
}

/// 分配给单个 VerificationUnit 的预算额度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationBudgetGrant {
    pub max_tokens: Option<u64>,
    pub max_wall_time_ms: Option<u64>,
    pub max_cost_microcents: Option<u64>,
}
```

### 2.4 文件: `src/verification_session.rs`

```rust
//! VerificationSession — 可恢复的验证会话

use serde::{Deserialize, Serialize};
use crate::ids::{VerificationSessionId, VerificationPlanId};
use crate::hash::ContentHash;
use std::collections::HashMap;

/// 可恢复的验证会话。
///
/// 恢复不变量:
///   - 合约哈希不变
///   - 规则包哈希不变
///   - Verifier 版本哈希不变
///   - target 的 content_hash 不变
///   → 旧结果复用
///   → 否则失效
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSession {
    pub session_id: VerificationSessionId,
    pub plan_id: VerificationPlanId,

    /// 绑定此会话的合约哈希
    pub contract_hash: ContentHash,
    /// 绑定此会话的规则包哈希
    pub rule_pack_hash: ContentHash,
    /// 绑定此会话的 verifier 版本哈希
    pub verifier_version_hash: ContentHash,

    /// 已完成的单元: fingerprint → completed result
    pub completed_units: HashMap<ContentHash, CompletedUnit>,
    /// 待处理的 unit_id
    pub pending_units: Vec<crate::ids::VerificationUnitId>,
    /// 此会话中收集的发现候选
    pub finding_candidates: Vec<super::finding::FindingCandidate>,
    /// 预算使用情况
    pub budget_used: super::verification_budget::VerificationBudgetUsage,
}

/// 一个已完成单元的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedUnit {
    pub unit_id: crate::ids::VerificationUnitId,
    pub unit_fingerprint: ContentHash,
    pub verifier_output_hash: ContentHash,
    pub token_used: u64,
    pub duration_ms: u64,
}

impl VerificationSession {
    /// 检查是否可以复用旧结果——任一绑定变化则失效
    pub fn validate_bindings(
        &self,
        contract_hash: &ContentHash,
        rule_pack_hash: &ContentHash,
        verifier_version_hash: &ContentHash,
    ) -> Result<(), SessionBindingsMismatch> {
        let mut mismatches = Vec::new();
        if &self.contract_hash != contract_hash {
            mismatches.push("contract_hash");
        }
        if &self.rule_pack_hash != rule_pack_hash {
            mismatches.push("rule_pack_hash");
        }
        if &self.verifier_version_hash != verifier_version_hash {
            mismatches.push("verifier_version_hash");
        }
        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(SessionBindingsMismatch { fields: mismatches })
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("session bindings mismatch: {fields:?}")]
pub struct SessionBindingsMismatch {
    pub fields: Vec<&'static str>,
}
```

### 2.5 文件: `src/finding.rs`

```rust
//! FindingCandidate → ResolvedFinding → EvidenceRecord 的生命周期

use serde::{Deserialize, Serialize};
use crate::ids::{FindingId, VerifierId, RuleId, EvidenceId};
use crate::hash::ContentHash;

/// Verifier 产出的原始发现——不可直接升级为 Evidence。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingCandidate {
    pub finding_id: FindingId,
    pub verifier_id: VerifierId,
    pub rule_id: RuleId,
    pub target_ref: String,
    pub target_content_hash: ContentHash,

    /// Verifier 声称的位置 (可选——可能无法定位)
    pub claimed_location: Option<EvidenceLocationClaim>,

    /// 发现标题/摘要
    pub title: String,

    /// 发现详细描述
    pub description: String,

    /// 严重度
    pub severity: Severity,

    /// Verifier 对自身判断的置信度 (0.0–1.0)
    pub verifier_confidence: f64,

    /// 修复建议（如有）
    pub suggested_fix: Option<String>,

    /// Verifier 的推理过程（如有）
    pub rationale: Option<String>,

    /// Verifier 特定元数据
    pub verifier_metadata: serde_json::Value,
}

/// 经过位置解析后的发现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFinding {
    pub candidate: FindingCandidate,

    /// 确定性解析后的位置——若无法唯一定位则为 None
    pub resolved_location: Option<EvidenceLocation>,

    /// 定位方法
    pub resolution_method: LocationResolutionMethod,

    /// 位置匹配置信度 (0.0–1.0)
    /// 双通道匹配都成功 → 1.0
    /// 仅主通道成功 → 0.9
    /// 仅后备通道成功 → 0.7
    /// 未匹配 → 0.0
    pub location_confidence: f64,
}

/// 经过交叉验证和去重后的发现——可升级为 EvidenceRecord。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorroboratedFinding {
    pub resolved: ResolvedFinding,

    /// 是否有第二个 Verifier 独立确认
    pub cross_verified: bool,
    pub cross_verifier_id: Option<VerifierId>,

    /// 是否与已有发现重复
    pub is_duplicate: bool,
    pub duplicate_of: Option<FindingId>,

    /// 映射到的 Criterion
    pub mapped_criteria: Vec<crate::ids::CriterionId>,

    /// 升级后的 EvidenceRecord ID（若已升级）
    pub evidence_id: Option<EvidenceId>,
}

/// Verifier 声称的位置——在解析前不可信。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLocationClaim {
    pub path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub code_snippet: Option<String>,
    pub symbol_name: Option<String>,
    pub byte_offset: Option<u64>,
}

/// 经过确定性解析后的位置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceLocation {
    SourceCode {
        file_path: String,
        file_hash: ContentHash,
        start_line: u32,
        end_line: u32,
        /// 上下文哈希 — 用于检测位置漂移
        context_hash: ContentHash,
    },
    Document {
        doc_path: String,
        section_id: String,
        paragraph_hash: ContentHash,
    },
    Dataset {
        schema_name: String,
        table_name: String,
        column_name: Option<String>,
        row_key: Option<String>,
    },
    Workflow {
        workflow_id: String,
        node_id: String,
        transition_id: Option<String>,
    },
    ExternalResource {
        resource_type: String,
        resource_id: String,
        version: String,
    },
    Unresolved {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationResolutionMethod {
    /// 双通道(Hunk + FileContent)都成功
    DualChannel,
    /// 仅 Hunk 匹配成功
    HunkOnly,
    /// 仅 FileContent 扫描成功
    FileContentOnly,
    /// 无法定位
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}
```

### 2.6 文件: `src/verification_budget.rs`

```rust
//! VerificationBudget — 多维度预算

use serde::{Deserialize, Serialize};

/// 一次验证运行的全局预算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationBudget {
    pub max_total_tokens: Option<u64>,
    pub max_total_cost_microcents: Option<u64>,
    pub max_wall_time_ms: Option<u64>,
    pub max_parallel_units: u32,

    /// 最多调用多少次语义 Verifier（最贵的资源）
    pub max_semantic_invocations: u32,

    /// 单文件最大字节数（超过则跳过）
    pub max_target_size_bytes: u64,
}

impl Default for VerificationBudget {
    fn default() -> Self {
        Self {
            max_total_tokens: None,
            max_total_cost_microcents: None,
            max_wall_time_ms: None,
            max_parallel_units: num_cpus::get() as u32,
            max_semantic_invocations: 50,
            max_target_size_bytes: 2 * 1024 * 1024, // 2 MiB
        }
    }
}

/// 预算使用情况——持续追踪。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationBudgetUsage {
    pub total_tokens_used: u64,
    pub total_cost_microcents: u64,
    pub wall_time_ms: u64,
    pub semantic_invocations_used: u32,
    pub units_completed: u32,
    pub units_skipped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    WithinBudget,
    TokenBudgetExhausted,
    CostBudgetExhausted,
    TimeBudgetExhausted,
    SemanticInvocationBudgetExhausted,
}

impl VerificationBudget {
    /// 检查预算状态——预算耗尽绝对不能产生 PASS
    pub fn check(&self, usage: &VerificationBudgetUsage) -> BudgetStatus {
        if let Some(max) = self.max_total_tokens {
            if usage.total_tokens_used >= max {
                return BudgetStatus::TokenBudgetExhausted;
            }
        }
        if let Some(max) = self.max_total_cost_microcents {
            if usage.total_cost_microcents >= max {
                return BudgetStatus::CostBudgetExhausted;
            }
        }
        if let Some(max) = self.max_wall_time_ms {
            if usage.wall_time_ms >= max {
                return BudgetStatus::TimeBudgetExhausted;
            }
        }
        if usage.semantic_invocations_used >= self.max_semantic_invocations {
            return BudgetStatus::SemanticInvocationBudgetExhausted;
        }
        BudgetStatus::WithinBudget
    }
}
```

### 2.7 文件: `src/rule_binding.rs`

```rust
//! RuleBinding — 将规则包绑定到验证目标

use serde::{Deserialize, Serialize};
use crate::ids::{RulePackId, VerifierId, VerificationTargetId};
use crate::hash::ContentHash;

/// 一个规则包——版本化、可哈希的规则集合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulePack {
    pub pack_id: RulePackId,
    pub pack_hash: ContentHash,
    pub version: String,

    /// 规则包中的规则列表
    pub rules: Vec<Rule>,

    /// 此规则包适用的目标类型
    pub applicable_target_kinds: Vec<super::verification_target::TargetKind>,

    /// 需要哪些 Verifier 来执行此规则包
    pub required_verifier_ids: Vec<VerifierId>,

    /// 默认严重度
    pub default_severity: super::finding::Severity,
}

/// 单条规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub rule_id: crate::ids::RuleId,
    pub name: String,
    pub description: String,
    pub category: String,

    /// 规则内容——Verifier 可解释的形式
    /// 对 IronClaw Semantic Verifier: 自然语言提示
    /// 对 Policy Verifier: Rego/OPA 策略
    /// 对 Lint Verifier: clippy rule name
    pub content: RuleContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum RuleContent {
    /// 自然语言规则——供 LLM Verifier 使用
    NaturalLanguage {
        text: String,
        examples: Vec<String>,
    },
    /// 结构化模式——供确定性 Verifier 使用
    Pattern {
        pattern_type: String,
        pattern: String,
    },
    /// 策略语言——供 Policy Verifier 使用
    Policy {
        language: String,
        source: String,
    },
}

/// 将目标映射到适用规则的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleBinding {
    pub target_id: VerificationTargetId,
    pub bound_rules: Vec<RuleBindingRef>,
    pub bound_at: chrono::DateTime<chrono::Utc>,
}
```

---

### 2.8 文件: `src/language_profile.rs`

```rust
//! LanguageProfile — 将语言映射到 Verifier 集合、规则包、和构建系统。
//!
//! 这是多语言架构的核心路由表。
//! Verification Fabric 本身不关心语言；所有语言特定逻辑集中于此。

use serde::{Deserialize, Serialize};
use crate::ids::{VerifierId, RulePackId};
use std::collections::HashMap;

/// 一个语言的完整验证配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageProfile {
    pub language: String,
    pub extensions: Vec<String>,
    pub rule_pack_ids: Vec<RulePackId>,
    pub verifiers: LanguageVerifierSet,
    /// 例如 Go: ~1.5, Rust: ~1.3, Python: ~1.1 tokens/byte
    pub tokens_per_byte: f64,
    pub build_system: Option<BuildSystemInfo>,
    pub ignore_patterns: Vec<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageVerifierSet {
    pub semantic: Vec<VerifierId>,
    pub build: Option<VerifierId>,
    pub test: Option<VerifierId>,
    pub lint: Option<VerifierId>,
    pub sast: Option<VerifierId>,
    pub format_check: Option<VerifierId>,
    pub dependency_audit: Option<VerifierId>,
    pub extras: HashMap<String, VerifierId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSystemInfo {
    pub build_command: String,
    pub test_command: String,
    pub root_indicator: String,
}

/// 全局语言注册表 — 从文件扩展名 → LanguageProfile 的快速查找。
#[derive(Debug, Clone, Default)]
pub struct LanguageRegistry {
    profiles: HashMap<String, LanguageProfile>,
    extension_map: HashMap<String, String>,
}

impl LanguageRegistry {
    pub fn new(profiles: Vec<LanguageProfile>) -> Self {
        let mut profile_map = HashMap::new();
        let mut ext_map = HashMap::new();
        for p in profiles {
            for ext in &p.extensions {
                ext_map.insert(ext.clone(), p.language.clone());
            }
            profile_map.insert(p.language.clone(), p);
        }
        Self { profiles: profile_map, extension_map: ext_map }
    }

    pub fn resolve_by_path(&self, path: &str) -> Option<&LanguageProfile> {
        let ext = std::path::Path::new(path)
            .extension().and_then(|e| e.to_str()).unwrap_or();
        let lang = self.extension_map.get(ext)?;
        self.profiles.get(lang)
    }

    pub fn get(&self, language: &str) -> Option<&LanguageProfile> {
        self.profiles.get(language)
    }

    pub fn languages(&self) -> Vec<&str> {
        self.profiles.keys().map(|s| s.as_str()).collect()
    }
}

// ── 内置语言 Profiles ──

impl LanguageProfile {
    pub fn rust() -> Self { Self {
        language: "rust".into(), extensions: vec!["rs".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.3,
        build_system: Some(BuildSystemInfo { build_command: "cargo build --workspace".into(), test_command: "cargo test --workspace".into(), root_indicator: "Cargo.toml".into() }),
        ignore_patterns: vec!["target/".into()],
        metadata: serde_json::json!({"linter":"clippy","formatter":"rustfmt","dep_audit":"cargo-deny"}),
    }}

    pub fn go() -> Self { Self {
        language: "go".into(), extensions: vec!["go".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.5,
        build_system: Some(BuildSystemInfo { build_command: "go build ./...".into(), test_command: "go test ./...".into(), root_indicator: "go.mod".into() }),
        ignore_patterns: vec!["vendor/".into()],
        metadata: serde_json::json!({"linter":"golangci-lint","formatter":"gofmt","nil_check":"nilaway","vuln":"govulncheck"}),
    }}

    pub fn python() -> Self { Self {
        language: "python".into(), extensions: vec!["py".into(),"pyi".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.1,
        build_system: Some(BuildSystemInfo { build_command: "python -m compileall .".into(), test_command: "python -m pytest".into(), root_indicator: "pyproject.toml".into() }),
        ignore_patterns: vec!["__pycache__/".into(),".venv/".into(),".pytest_cache/".into()],
        metadata: serde_json::json!({"linter":"ruff","formatter":"black","type_checker":"mypy","sast":["bandit","safety"]}),
    }}

    pub fn typescript() -> Self { Self {
        language: "typescript".into(), extensions: vec!["ts".into(),"tsx".into(),"js".into(),"jsx".into(),"mjs".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.4,
        build_system: Some(BuildSystemInfo { build_command: "npx tsc --noEmit".into(), test_command: "npx jest".into(), root_indicator: "package.json".into() }),
        ignore_patterns: vec!["node_modules/".into(),"dist/".into(),".next/".into()],
        metadata: serde_json::json!({"linter":"eslint","formatter":"prettier","type_checker":"tsc","sast":["semgrep"]}),
    }}

    pub fn java() -> Self { Self {
        language: "java".into(), extensions: vec!["java".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.4,
        build_system: Some(BuildSystemInfo { build_command: "mvn compile -q".into(), test_command: "mvn test".into(), root_indicator: "pom.xml".into() }),
        ignore_patterns: vec!["target/".into()],
        metadata: serde_json::json!({"linter":"checkstyle","sast":["spotbugs","pmd"]}),
    }}

    pub fn cpp() -> Self { Self {
        language: "cpp".into(), extensions: vec!["c".into(),"h".into(),"cpp".into(),"hpp".into(),"cc".into(),"hh".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.3,
        build_system: Some(BuildSystemInfo { build_command: "cmake --build build".into(), test_command: "ctest --test-dir build".into(), root_indicator: "CMakeLists.txt".into() }),
        ignore_patterns: vec!["build/".into()],
        metadata: serde_json::json!({"linter":"clang-tidy","formatter":"clang-format","sast":["cppcheck","flawfinder"]}),
    }}

    pub fn protobuf() -> Self { Self {
        language: "protobuf".into(), extensions: vec!["proto".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.2, build_system: None, ignore_patterns: vec![],
        metadata: serde_json::json!({"linter":"buf lint","formatter":"buf format"}),
    }}

    pub fn sql() -> Self { Self {
        language: "sql".into(), extensions: vec!["sql".into()],
        rule_pack_ids: vec![], verifiers: LanguageVerifierSet::default(),
        tokens_per_byte: 1.1, build_system: None, ignore_patterns: vec![],
        metadata: serde_json::json!({"linter":"sqlfluff"}),
    }}

    pub fn all_builtins() -> Vec<Self> {
        vec![Self::rust(), Self::go(), Self::python(), Self::typescript(),
             Self::java(), Self::cpp(), Self::protobuf(), Self::sql()]
    }
}

impl Default for LanguageVerifierSet {
    fn default() -> Self {
        Self { semantic: vec![], build: None, test: None, lint: None,
               sast: None, format_check: None, dependency_audit: None, extras: HashMap::new() }
    }
}
```

---

## 3. 纯函数层 — onto-assurance-core 新增

### 3.1 文件: `src/verification/scope_coverage.rs`

```rust
//! ScopeCoverage — 范围覆盖证明（纯函数）
//!
//! 对应: OCR internal/diff/git.go + internal/config/rules/system_rules.go
//!
//! 核心不变量:
//!   SC-1: 所有发现的候选目标 ∈ (included ∪ excluded)
//!   SC-2: included ∩ excluded = ∅
//!   SC-3: 每个 excluded 有非空的 reason

use onto_assurance_types::scope_manifest::{
    ExclusionReason, ExcludedTarget, ScopeManifest, ScopedTarget,
};
use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use onto_assurance_types::hash::{ContentHash, HashDomain, HashPurpose};
use crate::canonical;

/// 对所有候选目标执行范围覆盖——产生 ScopeManifest。
///
/// 纯函数: 相同输入 → 相同输出。
pub fn resolve_scope(
    all_targets: &[VerificationTarget],
    include_patterns: &[String],
    exclude_patterns: &[String],
    max_file_size_bytes: u64,
    budget: &onto_assurance_types::verification_budget::VerificationBudget,
    resolver_version: onto_assurance_types::ids::VerifierVersion,
) -> ScopeManifest {
    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for target in all_targets {
        // 大小检查
        if target.size_bytes > max_file_size_bytes {
            excluded.push(ExcludedTarget {
                target_ref: target.target_ref.clone(),
                reason: ExclusionReason::NonReviewable,
            });
            continue;
        }

        // include/exclude 模式匹配
        if !matches_any_pattern(&target.target_ref, include_patterns) {
            excluded.push(ExcludedTarget {
                target_ref: target.target_ref.clone(),
                reason: ExclusionReason::NotMatched,
            });
            continue;
        }
        if matches_any_pattern(&target.target_ref, exclude_patterns) {
            excluded.push(ExcludedTarget {
                target_ref: target.target_ref.clone(),
                reason: ExclusionReason::FilteredOut,
            });
            continue;
        }

        included.push(ScopedTarget {
            target_id: target.target_id,
            target_ref: target.target_ref.clone(),
            target_kind: target.target_kind,
            risk_level: target.risk_level,
        });
    }

    let manifest_hash = compute_scope_hash(&included, &excluded, &resolver_version);
    ScopeManifest {
        included_targets: included,
        excluded_targets: excluded,
        resolver_version,
        manifest_hash,
    }
}

/// Glob 匹配——移植自 OCR 的 doublestar/v4
fn matches_any_pattern(target_ref: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true; // 空 = 匹配全部
    }
    patterns.iter().any(|p| glob_match(p, target_ref))
}

/// glob 匹配实现 — 移植自 Go doublestar/v4
/// 见 §6.3 Go→Rust 移植
fn glob_match(pattern: &str, path: &str) -> bool {
    // 移植 Go doublestar/v4 的核心逻辑
    // 支持的语法:
    //   *      — 匹配单层（不含 /）
    //   **     — 匹配任意深度
    //   ?      — 匹配单个字符
    //   [abc]  — 字符集
    //   {a,b}  — 可选
    todo!("§6.3: 移植 Go doublestar/v4 到 Rust glob_match")
}

fn compute_scope_hash(
    included: &[ScopedTarget],
    excluded: &[ExcludedTarget],
    version: &onto_assurance_types::ids::VerifierVersion,
) -> ContentHash {
    let domain = HashDomain::new(HashPurpose::Content, "SCOPE_MANIFEST");
    // 规范 JSON 序列化 + SHA-256
    // 使用已有的 canonical 模块
    canonical::compute_hash(&(included, excluded, version), &domain)
        .expect("scope manifest serialization infallible")
}
```

### 3.2 文件: `src/verification/rule_binding.rs`

```rust
//! RuleBinding — 确定性规则路由（纯函数）
//!
//! 对应: OCR internal/config/rules/system_rules.go Resolver.Resolve(path)
//!
//! 差异: OCR 只按 path 匹配。此处泛化为 TargetKind + language + risk_level + metadata

use onto_assurance_types::rule_binding::{Rule, RuleBinding, RulePack};
use onto_assurance_types::verification_target::VerificationTarget;
use onto_assurance_types::ids::VerificationTargetId;

/// 将一组 RulePack 绑定到一个 VerificationTarget。
///
/// 纯函数: 相同 target + 相同 packs → 相同 binding。
pub fn bind_rules(
    target: &VerificationTarget,
    rule_packs: &[RulePack],
) -> RuleBinding {
    let mut bound = Vec::new();

    for pack in rule_packs {
        // 检查 target_kind 是否适用
        if !pack.applicable_target_kinds.contains(&target.target_kind) {
            continue;
        }

        // 对 pack 中的每条规则检查是否适用
        for rule in &pack.rules {
            if rule_applies(rule, target) {
                bound.push(onto_assurance_types::verification_plan::RuleBindingRef {
                    rule_pack_id: pack.pack_id.to_string(),
                    rule_pack_hash: pack.pack_hash.clone(),
                    verifier_id: pack.required_verifier_ids.first()
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }
    }

    RuleBinding {
        target_id: target.target_id,
        bound_rules: bound,
        bound_at: chrono::Utc::now(),
    }
}

fn rule_applies(rule: &Rule, target: &VerificationTarget) -> bool {
    // 确定性匹配逻辑:
    // 1. category 匹配 target_kind
    // 2. language 匹配（若 rule 指定了 language）
    // 3. 自定义 metadata 条件
    //
    // 移植自 OCR SystemRule.Resolve() 的 glob 匹配逻辑
    // 见 §6.4
    todo!("§6.4: 移植 OCR 规则匹配逻辑到 Rust")
}
```

### 3.3 文件: `src/verification/location_binding.rs`

```rust
//! LocationBinding — 确定性位置解析（纯函数）
//!
//! 对应: OCR internal/diff/resolver.go ResolveLineNumbers()
//!
//! 双通道算法:
//!   主通道: 在 diff hunk 中匹配代码片段 → 精确行号
//!   后备通道: 在全文件内容中逐行扫描 → 近似行号

use onto_assurance_types::finding::{
    EvidenceLocation, EvidenceLocationClaim, LocationResolutionMethod, ResolvedFinding,
};
use onto_assurance_types::hash::ContentHash;

/// 对一批 FindingCandidate 执行位置解析。
///
/// 纯函数: 相同 findings + 相同 targets → 相同 resolved locations。
pub fn resolve_locations(
    candidates: &[onto_assurance_types::finding::FindingCandidate],
    target_contents: &[TargetContent],
) -> Vec<ResolvedFinding> {
    candidates
        .iter()
        .map(|c| resolve_single(c, target_contents))
        .collect()
}

/// 对单个 FindingCandidate 执行双通道位置解析。
fn resolve_single(
    candidate: &onto_assurance_types::finding::FindingCandidate,
    target_contents: &[TargetContent],
) -> ResolvedFinding {
    let claim = match &candidate.claimed_location {
        Some(c) => c,
        None => {
            return ResolvedFinding {
                candidate: candidate.clone(),
                resolved_location: None,
                resolution_method: LocationResolutionMethod::Failed,
                location_confidence: 0.0,
            };
        }
    };

    // 找到对应的 target content
    let content = match target_contents.iter().find(|tc| tc.target_ref == claim.path) {
        Some(c) => c,
        None => {
            return ResolvedFinding {
                candidate: candidate.clone(),
                resolved_location: None,
                resolution_method: LocationResolutionMethod::Failed,
                location_confidence: 0.0,
            };
        }
    };

    let snippet = claim.code_snippet.as_deref().unwrap_or("");
    if snippet.is_empty() {
        return ResolvedFinding {
            candidate: candidate.clone(),
            resolved_location: None,
            resolution_method: LocationResolutionMethod::Failed,
            location_confidence: 0.0,
        };
    }

    // ── 主通道: Diff hunk 匹配 ──
    if let Some(loc) = resolve_from_diff_hunks(content, snippet) {
        return ResolvedFinding {
            candidate: candidate.clone(),
            resolved_location: Some(loc),
            resolution_method: LocationResolutionMethod::DualChannel,
            location_confidence: 1.0,
        };
    }

    // ── 后备通道: 全文件内容扫描 ──
    if let Some(loc) = resolve_from_full_content(content, snippet) {
        let confidence = if content.has_diff {
            0.7 // 后备通道 + 有 diff → 中等置信
        } else {
            0.9 // 后备通道 + 无 diff → 高置信
        };
        return ResolvedFinding {
            candidate: candidate.clone(),
            resolved_location: Some(loc),
            resolution_method: LocationResolutionMethod::FileContentOnly,
            location_confidence,
        };
    }

    ResolvedFinding {
        candidate: candidate.clone(),
        resolved_location: None,
        resolution_method: LocationResolutionMethod::Failed,
        location_confidence: 0.0,
    }
}

/// Diff hunk 中的位置信息。
struct DiffHunkLine {
    line_num: u32,
    content: String,
    side: DiffSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSide {
    Context,
    Added,
    Deleted,
}

/// 目标内容——可能是完整文件或 diff。
struct TargetContent {
    target_ref: String,
    /// 完整文件内容（若无 diff 则仅此字段有值）
    full_content: String,
    /// diff 内容（unified diff 格式）
    diff_content: Option<String>,
    /// diff hunks 的解析结果
    hunks: Vec<Vec<DiffHunkLine>>,
    has_diff: bool,
}

/// 主通道: 从 diff hunk 中匹配（移植自 OCR resolveFromHunk）
fn resolve_from_diff_hunks(
    content: &TargetContent,
    snippet: &str,
) -> Option<EvidenceLocation> {
    if !content.has_diff {
        return None;
    }

    let target_lines = normalize_and_split(snippet);
    if target_lines.is_empty() {
        return None;
    }

    // 先尝试 new-side (added + context)
    for hunk in &content.hunks {
        if let Some((start, end)) = match_consecutive(hunk, &target_lines) {
            return Some(EvidenceLocation::SourceCode {
                file_path: content.target_ref.clone(),
                file_hash: ContentHash::new("todo"),
                start_line: start,
                end_line: end,
                context_hash: ContentHash::new("todo"),
            });
        }
    }

    None
}

/// 后备通道: 在全文件内容中逐行扫描（移植自 OCR resolveFromFileContent）
fn resolve_from_full_content(
    content: &TargetContent,
    snippet: &str,
) -> Option<EvidenceLocation> {
    let target_lines = normalize_and_split(snippet);
    if target_lines.is_empty() {
        return None;
    }

    let file_lines: Vec<&str> = content.full_content.lines().collect();
    if file_lines.len() < target_lines.len() {
        return None;
    }

    // 滑动窗口匹配（跳过空行）
    let normalized: Vec<(u32, String)> = file_lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let n = normalize_line(line);
            if n.is_empty() {
                None
            } else {
                Some((i as u32 + 1, n))
            }
        })
        .collect();

    for i in 0..=normalized.len().saturating_sub(target_lines.len()) {
        let mut matched = true;
        for (j, target) in target_lines.iter().enumerate() {
            if normalized[i + j].1 != *target {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(EvidenceLocation::SourceCode {
                file_path: content.target_ref.clone(),
                file_hash: ContentHash::new("todo"),
                start_line: normalized[i].0,
                end_line: normalized[i + target_lines.len() - 1].0,
                context_hash: ContentHash::new("todo"),
            });
        }
    }

    None
}

/// 在 hunk 行中连续匹配 target_lines（移植自 OCR matchConsecutive）
fn match_consecutive(
    hunk_lines: &[DiffHunkLine],
    target_lines: &[String],
) -> Option<(u32, u32)> {
    if target_lines.is_empty() || hunk_lines.len() < target_lines.len() {
        return None;
    }

    for i in 0..=hunk_lines.len() - target_lines.len() {
        let mut matched = true;
        for (j, target) in target_lines.iter().enumerate() {
            if normalize_line(&hunk_lines[i + j].content) != *target {
                matched = false;
                break;
            }
        }
        if matched {
            return Some((
                hunk_lines[i].line_num,
                hunk_lines[i + target_lines.len() - 1].line_num,
            ));
        }
    }

    None
}

/// 行规范化——移植自 OCR normalizeLine
fn normalize_line(line: &str) -> String {
    let s = line.trim();
    let s = s.trim_start_matches('+');
    let s = s.trim_start_matches('-');
    s.trim().to_string()
}

/// 将代码片段拆分为规范化行
fn normalize_and_split(code: &str) -> Vec<String> {
    code.lines()
        .map(normalize_line)
        .filter(|s| !s.is_empty())
        .collect()
}
```

### 3.4 文件: `src/verification/finding_normalization.rs`

```rust
//! FindingNormalization — 发现去重、交叉验证、Criterion 映射（纯函数）

use onto_assurance_types::finding::{
    CorroboratedFinding, FindingCandidate, ResolvedFinding,
};
use onto_assurance_types::ids::{CriterionId, FindingId, VerifierId};
use std::collections::HashSet;

/// 去重: 相同 target + 相同位置 + 相似 claim → 标记为重复
///
/// 纯函数。
pub fn deduplicate(resolved: &[ResolvedFinding]) -> Vec<CorroboratedFinding> {
    let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
    let mut result = Vec::new();

    for rf in resolved {
        let key = match &rf.resolved_location {
            Some(loc) => location_key(loc),
            None => continue,
        };

        let is_dup = !seen.insert(key);
        result.push(CorroboratedFinding {
            resolved: rf.clone(),
            cross_verified: false,
            cross_verifier_id: None,
            is_duplicate: is_dup,
            duplicate_of: None, // 可在后续阶段填充
            mapped_criteria: Vec::new(),
            evidence_id: None,
        });
    }

    result
}

fn location_key(loc: &onto_assurance_types::finding::EvidenceLocation) -> (String, u32, u32) {
    match loc {
        onto_assurance_types::finding::EvidenceLocation::SourceCode {
            file_path,
            start_line,
            end_line,
            ..
        } => (file_path.clone(), *start_line, *end_line),
        onto_assurance_types::finding::EvidenceLocation::Document {
            doc_path,
            section_id,
            ..
        } => (format!("{}#{}", doc_path, section_id), 0, 0),
        onto_assurance_types::finding::EvidenceLocation::Dataset {
            schema_name,
            table_name,
            column_name,
            ..
        } => (
            format!("{}.{}.{}", schema_name, table_name, column_name.as_deref().unwrap_or("*")),
            0,
            0,
        ),
        onto_assurance_types::finding::EvidenceLocation::Workflow {
            workflow_id,
            node_id,
            ..
        } => (format!("{}:{}", workflow_id, node_id), 0, 0),
        onto_assurance_types::finding::EvidenceLocation::ExternalResource {
            resource_type,
            resource_id,
            ..
        } => (format!("{}:{}", resource_type, resource_id), 0, 0),
        onto_assurance_types::finding::EvidenceLocation::Unresolved { .. } => {
            (String::new(), 0, 0)
        }
    }
}

/// 交叉验证: 由多个 Verifier 独立确认的发现 → 置信度更高
///
/// 纯函数。
pub fn cross_verify(
    findings: &[CorroboratedFinding],
    secondary_verifier_results: &[FindingCandidate],
) -> Vec<CorroboratedFinding> {
    // 对每个 finding，检查是否有来自不同 Verifier 的独立确认
    todo!("§6.5")
}

/// 将 CorroboratedFinding 映射到 Contract Criterion。
///
/// 纯函数。
pub fn map_to_criteria(
    findings: &[CorroboratedFinding],
    criteria: &[onto_assurance_types::contract::Criterion],
) -> Vec<CorroboratedFinding> {
    // 规则: Finding.category / Finding.severity / target_ref → Criterion
    todo!("§6.5")
}
```

### 3.5 文件: `src/verification/mod.rs`

```rust
pub mod scope_coverage;
pub mod rule_binding;
pub mod location_binding;
pub mod finding_normalization;
```

---

## 4. 运行时层 — onto-assurance-runtime 新增

### 4.1 文件: `src/verification/coordinator.rs`

```rust
//! VerificationCoordinator — 完整的验证运行编排器
//!
//! 组合:
//!   ScopeResolver → RuleRouter → Planner → Scheduler → SessionManager
//!   → VerifierInvocation → LocationResolver → EvidenceBuilder

use async_trait::async_trait;
use onto_assurance_core::verification::{
    scope_coverage, rule_binding, location_binding, finding_normalization,
};
use onto_assurance_types::scope_manifest::ScopeManifest;
use onto_assurance_types::verification_plan::{VerificationPlan, VerificationUnit};
use onto_assurance_types::verification_session::VerificationSession;
use onto_assurance_types::verification_budget::{VerificationBudget, VerificationBudgetUsage};
use onto_assurance_types::finding::{FindingCandidate, CorroboratedFinding};
use onto_assurance_types::hash::ContentHash;
use std::sync::Arc;

use super::planner::PlanGenerator;
use super::scheduler::UnitScheduler;
use super::session::SessionManager;
use super::budget::BudgetTracker;
use super::locator::LocationResolverService;
use super::evidence_builder::EvidenceBuilderService;

/// VerificationCoordinator 是"ocr review"的本体——
/// 但不做 LLM 调用，只做编排。
pub struct VerificationCoordinator {
    planner: Arc<dyn PlanGenerator>,
    scheduler: Arc<dyn UnitScheduler>,
    session_manager: Arc<dyn SessionManager>,
    budget_tracker: Arc<BudgetTracker>,
    location_resolver: Arc<LocationResolverService>,
    evidence_builder: Arc<EvidenceBuilderService>,
}

impl VerificationCoordinator {
    /// 执行一次完整的验证运行。
    pub async fn run(
        &self,
        targets: Vec<onto_assurance_types::verification_target::VerificationTarget>,
        rule_packs: Vec<onto_assurance_types::rule_binding::RulePack>,
        budget: VerificationBudget,
        contract: &onto_assurance_types::contract::ExecutionContract,
        resume_session_id: Option<onto_assurance_types::ids::VerificationSessionId>,
    ) -> Result<VerificationRunResult, VerificationError> {
        // ── 1. 范围确定 ──
        let scope = scope_coverage::resolve_scope(
            &targets,
            &[], // include_patterns — 从 contract/profile 派生
            &[], // exclude_patterns
            budget.max_target_size_bytes,
            &budget,
            onto_assurance_types::ids::VerifierVersion::new("1.0.0"),
        );

        // ── 2. 规则路由 ──
        let bindings: Vec<_> = scope
            .included_targets
            .iter()
            .filter_map(|st| {
                targets.iter().find(|t| t.target_id == st.target_id)
            })
            .map(|target| rule_binding::bind_rules(target, &rule_packs))
            .collect();

        // ── 3. 计划生成 ──
        let mut plan = self.planner.generate(
            &scope,
            &bindings,
            &budget,
        ).await?;

        // ── 4. Session 恢复 ──
        if let Some(sid) = resume_session_id {
            if let Some(session) = self.session_manager.load(&sid).await? {
                plan = self.session_manager.apply_resume(plan, &session)?;
            }
        }

        // ── 5. 调度执行 ──
        let mut all_findings: Vec<FindingCandidate> = Vec::new();
        let mut usage = VerificationBudgetUsage::default();

        for unit in &plan.units {
            // 若 session 中已完成，跳过
            if self.session_manager.is_unit_completed(unit) {
                continue;
            }

            let unit_findings = self.scheduler.execute_unit(unit, &budget).await?;
            all_findings.extend(unit_findings);
        }

        // ── 6. 位置解析 ──
        let target_contents = self.load_target_contents(&scope).await?;
        let resolved = location_binding::resolve_locations(&all_findings, &target_contents);

        // ── 7. 去重 + 交叉验证 ──
        let corroborated = finding_normalization::deduplicate(&resolved);

        // ── 8. Criterion 映射 ──
        let mapped = finding_normalization::map_to_criteria(
            &corroborated,
            &contract.criteria,
        );

        // ── 9. 证据构建 ──
        let evidence_records = self.evidence_builder.build(&mapped).await?;

        Ok(VerificationRunResult {
            scope,
            plan,
            findings: mapped,
            evidence_records,
            budget_usage: usage,
        })
    }

    async fn load_target_contents(
        &self,
        scope: &ScopeManifest,
    ) -> Result<Vec<onto_assurance_core::verification::location_binding::TargetContent>, VerificationError> {
        todo!("从 artifact store 加载每个 target 的内容")
    }
}

#[derive(Debug, Clone)]
pub struct VerificationRunResult {
    pub scope: ScopeManifest,
    pub plan: VerificationPlan,
    pub findings: Vec<CorroboratedFinding>,
    pub evidence_records: Vec<onto_assurance_types::evidence::EvidenceRecord>,
    pub budget_usage: VerificationBudgetUsage,
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("scope error: {0}")]
    Scope(String),
    #[error("plan error: {0}")]
    Plan(String),
    #[error("verifier error: {0}")]
    Verifier(String),
    #[error("budget exhausted: {0:?}")]
    BudgetExhausted(onto_assurance_types::verification_budget::BudgetStatus),
    #[error("session error: {0}")]
    Session(String),
}
```

### 4.2 文件: `src/verification/planner.rs`

```rust
//! PlanGenerator — 将 Scope + RuleBindings 分解为 VerificationUnit

use async_trait::async_trait;
use onto_assurance_types::scope_manifest::ScopeManifest;
use onto_assurance_types::verification_plan::{VerificationPlan, VerificationUnit};
use onto_assurance_types::rule_binding::RuleBinding;
use onto_assurance_types::verification_budget::VerificationBudget;
use onto_assurance_types::ids::{VerificationPlanId, VerificationUnitId};
use onto_assurance_types::hash::{ContentHash, HashDomain, HashPurpose};
use onto_assurance_core::canonical;

#[async_trait]
pub trait PlanGenerator: Send + Sync {
    async fn generate(
        &self,
        scope: &ScopeManifest,
        bindings: &[RuleBinding],
        budget: &VerificationBudget,
    ) -> Result<VerificationPlan, PlanError>;
}

/// 默认 PlanGenerator — 按模块/包分组（移植自 OCR 的 file bundling）
pub struct DefaultPlanGenerator {
    /// 每个 VerificationUnit 最大目标数
    max_targets_per_unit: usize,
}

impl DefaultPlanGenerator {
    pub fn new(max_targets_per_unit: usize) -> Self {
        Self { max_targets_per_unit }
    }

    /// 分组策略——移植自 OCR agent 的文件打包逻辑:
    ///   1. 按目录分组（同目录文件依赖关联强）
    ///   2. 关联文件合并（如 .properties 多语言文件）
    ///   3. 每个组不超过 max_targets_per_unit
    fn group_by_module(&self, scope: &ScopeManifest) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut current_group: Vec<String> = Vec::new();
        let mut current_dir: Option<String> = None;

        for target in &scope.included_targets {
            let dir = parent_dir(&target.target_ref);

            match &current_dir {
                Some(d) if d == &dir && current_group.len() < self.max_targets_per_unit => {
                    current_group.push(target.target_ref.clone());
                }
                _ => {
                    if !current_group.is_empty() {
                        groups.push(std::mem::take(&mut current_group));
                    }
                    current_dir = Some(dir);
                    current_group.push(target.target_ref.clone());
                }
            }
        }

        if !current_group.is_empty() {
            groups.push(current_group);
        }

        groups
    }
}

fn parent_dir(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

#[async_trait]
impl PlanGenerator for DefaultPlanGenerator {
    async fn generate(
        &self,
        scope: &ScopeManifest,
        bindings: &[RuleBinding],
        budget: &VerificationBudget,
    ) -> Result<VerificationPlan, PlanError> {
        let unit_groups = self.group_by_module(scope);

        let units: Vec<VerificationUnit> = unit_groups
            .into_iter()
            .map(|target_refs| {
                let rule_bindings: Vec<_> = bindings
                    .iter()
                    .filter(|b| target_refs.contains(&b.target_id.to_string()))
                    .flat_map(|b| b.bound_rules.clone())
                    .collect();

                let fingerprint = compute_unit_fingerprint(&target_refs, &rule_bindings);

                VerificationUnit {
                    unit_id: VerificationUnitId::new(),
                    target_refs,
                    rule_bindings,
                    dependency_refs: Vec::new(),
                    budget_grant: onto_assurance_types::verification_plan::VerificationBudgetGrant {
                        max_tokens: None,
                        max_wall_time_ms: None,
                        max_cost_microcents: None,
                    },
                    unit_fingerprint: fingerprint,
                }
            })
            .collect();

        Ok(VerificationPlan {
            plan_id: VerificationPlanId::new(),
            manifest_hash: scope.manifest_hash.clone(),
            units,
            total_estimated_tokens: 0, // 由 estimator 填充
            max_parallel_units: budget.max_parallel_units,
        })
    }
}

fn compute_unit_fingerprint(
    target_refs: &[String],
    rule_bindings: &[onto_assurance_types::verification_plan::RuleBindingRef],
) -> ContentHash {
    let domain = HashDomain::new(HashPurpose::Content, "VERIFICATION_UNIT");
    canonical::compute_hash(&(target_refs, rule_bindings), &domain)
        .expect("unit fingerprint infallible")
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("scope empty")]
    EmptyScope,
    #[error("no rules applicable")]
    NoRulesApplicable,
}
```

### 4.3 文件: `src/verification/mod.rs`

```rust
pub mod coordinator;
pub mod planner;
pub mod scheduler;
pub mod session;
pub mod budget;
pub mod locator;
pub mod evidence_builder;
```

---

## 5. 代码领域实现 — onto-code-pack

### 5.1 目录结构

```
crates/onto-code-pack/src/
├── lib.rs                     # LanguageRegistry 初始化 + 注册所有内置 profiles
├── scope/
│   ├── mod.rs
│   ├── diff.rs                # git diff → VerificationTarget[] (带语言路由)
│   ├── repository_scan.rs     # 全仓库扫描 → VerificationTarget[]
│   └── polyglot.rs            # 多语言仓库: 按语言分片
├── rules/
│   ├── mod.rs                 # RulePackLoader: 按 language 加载规则包
│   ├── rust.rs                # Rust 规则包 (clippy + unsafe + ownership)
│   ├── go.rs                  # Go 规则包 (goroutine leak + nil pointer + defer)
│   ├── python.rs              # Python 规则包 (type safety + async + security)
│   ├── typescript.rs          # TS 规则包 (null safety + promise + injection)
│   ├── java.rs                # Java 规则包 (concurrency + SQL injection + JPA)
│   ├── cpp.rs                 # C++ 规则包 (memory safety + UB + RAII)
│   ├── protobuf.rs            # Protobuf 规则包 (breaking changes + naming)
│   └── generic.rs             # 通用规则包 (所有语言通用的安全/架构规则)
├── verifiers/
│   ├── mod.rs                 # VerifierRegistry: 按 LanguageProfile 选择 Verifier
│   ├── deterministic/
│   │   ├── mod.rs
│   │   ├── build.rs           # 分发: cargo build / go build / tsc / mvn / cmake
│   │   ├── test.rs            # 分发: cargo test / go test / pytest / jest
│   │   ├── lint.rs            # 分发: clippy / golangci-lint / ruff / eslint
│   │   ├── format_check.rs    # 分发: rustfmt / gofmt / black / prettier
│   │   ├── sast.rs            # 分发: semgrep / codeql / bandit / gosec
│   │   └── dependency_audit.rs# 分发: cargo-deny / govulncheck / safety / npm audit
│   ├── semantic/
│   │   ├── mod.rs
│   │   ├── ironclaw_semantic.rs # IronClaw → SemanticVerifierPort (原全能代理)
│   │   ├── generic_llm.rs     # 通用 LLM Verifier (无 IronClaw 时，Claude/GPT 直连)
│   │   └── prompt_builder.rs  # 按语言构建提示: Rust prompt ≠ Go prompt ≠ Python prompt
│   └── registry.rs            # VerifierRegistry: LanguageProfile → [Verifier]
├── location/
│   ├── mod.rs
│   ├── source_code.rs         # 行号定位 (移植自 OCR diff/resolver.go)
│   ├── ast.rs                 # AST 节点定位: tree-sitter (支持所有语言)
│   └── symbol.rs              # 符号级定位: LSP / ctags
└── session/
    ├── mod.rs
    └── jsonl.rs               # JSONL 持久化 (移植自 OCR session/persist.go)
```

### 5.2 文件: `src/lib.rs` — 多语言初始化

```rust
//! onto-code-pack — 代码领域的 Verification Fabric 实现
//!
//! 职责:
//!   1. 注册所有支持语言的 LanguageProfile
//!   2. 注册所有确定性 Verifier 实现
//!   3. 注册语义 Verifier 实现 (IronClaw + 通用 LLM)
//!   4. 暴露统一的 VerificationCoordinator

use onto_assurance_types::language_profile::{LanguageProfile, LanguageRegistry};

pub mod scope;
pub mod rules;
pub mod verifiers;
pub mod location;
pub mod session;

/// 构建默认的 LanguageRegistry — 包含所有内置语言。
pub fn default_language_registry() -> LanguageRegistry {
    LanguageRegistry::new(LanguageProfile::all_builtins())
}

/// 构建仅包含指定语言的 LanguageRegistry。
pub fn language_registry_for(languages: &[&str]) -> LanguageRegistry {
    let all = LanguageProfile::all_builtins();
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|p| languages.contains(&p.language.as_str()))
        .collect();
    LanguageRegistry::new(filtered)
}
```

### 5.3 文件: `src/scope/diff.rs` — 带语言路由的 Diff Scope Provider

```rust
//! Git diff → VerificationTarget[] (带语言路由)
//! 移植自 OCR internal/diff/git.go + 增强多语言支持

use onto_assurance_types::language_profile::LanguageRegistry;
use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use onto_assurance_types::hash::ContentHash;
use onto_assurance_types::ids::VerificationTargetId;

pub struct DiffScopeProvider {
    repo_dir: String,
    from_ref: Option<String>,
    to_ref: Option<String>,
    commit: Option<String>,
    /// 语言注册表 — 用于自动检测文件语言
    language_registry: LanguageRegistry,
}

impl DiffScopeProvider {
    pub fn new_workspace(repo_dir: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: None, to_ref: None,
               commit: None, language_registry: registry }
    }

    pub fn new_range(repo_dir: &str, from: &str, to: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: Some(from.into()),
               to_ref: Some(to.into()), commit: None, language_registry: registry }
    }

    pub fn new_commit(repo_dir: &str, commit: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: None, to_ref: None,
               commit: Some(commit.into()), language_registry: registry }
    }

    /// 枚举变更文件 — 返回 VerificationTarget 列表 (带语言信息)
    pub fn enumerate(&self) -> Result<Vec<VerificationTarget>, DiffError> {
        let files = self.run_git_diff()?;
        let ignored = self.aggregate_ignored_patterns();
        let targets: Vec<_> = files
            .into_iter()
            .filter(|f| !ignored.iter().any(|i| f.starts_with(i)))
            .filter_map(|path| self.file_to_target(&path))
            .collect();

        // 统计语言分布 (用于日志/遥测)
        let mut lang_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for t in &targets {
            if let Some(lang) = &t.language {
                *lang_counts.entry(lang.clone()).or_default() += 1;
            }
        }
        tracing::info!(targets = targets.len(), ?lang_counts, "diff scope enumerated");

        Ok(targets)
    }

    fn file_to_target(&self, path: &str) -> Option<VerificationTarget> {
        let full_path = std::path::Path::new(&self.repo_dir).join(path);
        let content = std::fs::read(&full_path).ok()?;

        // 二进制检测 — 移植自 OCR scan provider
        if content.iter().take(8000).any(|&b| b == 0x00) {
            return None;
        }

        // 语言检测 — 通过 LanguageRegistry
        let profile = self.language_registry.resolve_by_path(path);
        let language = profile.map(|p| p.language.clone());

        // 大小检查 — 按语言的 token/byte 比例计算上限
        let tokens_per_byte = profile.map(|p| p.tokens_per_byte).unwrap_or(1.5);
        let max_bytes = (200_000.0 / tokens_per_byte) as u64; // ~200K tokens max
        if content.len() as u64 > max_bytes {
            return None;
        }

        let hash = ContentHash::from_bytes(&sha2::Sha256::digest(&content));

        Some(VerificationTarget {
            target_id: VerificationTargetId::new(),
            target_kind: TargetKind::SourceFile,
            target_ref: path.to_string(),
            content_hash: hash,
            size_bytes: content.len() as u64,
            language,
            risk_level: onto_assurance_types::enums::RiskLevel::Medium,
            metadata: serde_json::json!({
                "extension": std::path::Path::new(path)
                    .extension().and_then(|e| e.to_str()),
            }),
        })
    }

    /// 聚合所有语言的 ignore_patterns + 通用忽略路径
    fn aggregate_ignored_patterns(&self) -> Vec<String> {
        let mut patterns = vec![
            ".idea/".into(), ".vscode/".into(), ".svn/".into(), ".git/".into(),
        ];
        for lang in self.language_registry.languages() {
            if let Some(p) = self.language_registry.get(lang) {
                patterns.extend(p.ignore_patterns.clone());
            }
        }
        patterns
    }

    fn run_git_diff(&self) -> Result<Vec<String>, DiffError> {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir).arg("diff").arg("--name-only");

        if let Some(commit) = &self.commit {
            cmd.arg(&format!("{}^..{}", commit, commit));
        } else if let (Some(from), Some(to)) = (&self.from_ref, &self.to_ref) {
            cmd.arg(&format!("{}..{}", from, to));
        } else {
            cmd.arg("--staged");
        }

        let output = cmd.output().map_err(|e| DiffError::Git(e.to_string()))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines().map(|s| s.to_string())
            .filter(|s| !s.is_empty()).collect())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("git error: {0}")]
    Git(String),
    #[error("not a git repository: {0}")]
    NotGitRepo(String),
}
```

### 5.4 各语言规则包 — 具体规则内容

#### 5.4.1 `src/rules/rust.rs`

```rust
//! Rust 规则包 — 移植自 IronClaw 的 review-discipline.md + clippy 规则

use onto_assurance_types::rule_binding::{RulePack, Rule, RuleContent};
use onto_assurance_types::ids::{RulePackId, VerifierId, RuleId};
use onto_assurance_types::hash::{ContentHash, HashDomain, HashPurpose};
use onto_assurance_core::canonical;

pub fn rust_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "no-unwrap-in-production".into(),
            description: "禁止在非测试代码中使用 .unwrap() / .expect()".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "不允许 .unwrap() 和 .expect() 出现在 src/ 的任何文件中。\
                       使用 thiserror 定义错误类型，用 ? 传播错误，\
                       用 match 或 if let 处理 Option。测试代码中的 unwrap 是允许的。".into(),
                examples: vec![
                    "Bad: let x = foo().unwrap();".into(),
                    "Good: let x = foo().map_err(|e| MyError::Foo(e))?;".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "unsafe-audit".into(),
            description: "所有 unsafe 块必须有 SAFETY: 注释".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "每个 unsafe 块必须包含 SAFETY: 注释，解释为什么该块是安全的。\
                       如果你不确定是否可以移除 unsafe，请标记为需要人工审查。".into(),
                examples: vec![
                    "// SAFETY: This pointer is valid because...".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "trait-impl-consistency".into(),
            description: "双后端 (PostgreSQL + libSQL) 的 trait 实现必须一致".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "如果代码库有双后端 (如 PostgreSQL 和 libSQL)，\
                       对 trait 的任何修改必须在两个实现中同步更新。\
                       检查 postgres.rs 和 libsql_backend.rs。".into(),
                examples: vec![],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "clippy-all-warnings".into(),
            description: "cargo clippy --all-targets --all-features -- -D warnings".into(),
            category: "style".into(),
            content: RuleContent::Pattern {
                pattern_type: "clippy".into(),
                pattern: "clippy::all,clippy::pedantic,clippy::nursery".into(),
            },
        },
    ];

    let pack_hash = compute_pack_hash(&rules);
    RulePack {
        pack_id: RulePackId::new(),
        pack_hash,
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}

fn compute_pack_hash(rules: &[Rule]) -> ContentHash {
    let domain = HashDomain::new(HashPurpose::Content, "RULE_PACK");
    canonical::compute_hash(rules, &domain).expect("rule pack hash infallible")
}
```

#### 5.4.2 `src/rules/go.rs`

```rust
//! Go 规则包 — 从 OpenCodeReview 的 Go 审查经验 + golangci-lint 规则

pub fn go_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "goroutine-leak".into(),
            description: "goroutine 泄漏 — 确保每个 goroutine 有明确的退出路径".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查每个 goroutine:\n\
                       1. 是否有 context.Context 可以取消?\n\
                       2. 是否有 channel close 信号?\n\
                       3. 是否有 select + default 防止阻塞?\n\
                       特别注意: HTTP handler 中启动的 goroutine 必须在请求结束时退出。".into(),
                examples: vec![
                    "Bad: go func() { for { doWork() } }()  // never exits".into(),
                    "Good: go func() { for { select { case <-ctx.Done(): return } } }()".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "nil-interface-vs-nil-concrete".into(),
            description: "接口 nil ≠ 具体类型 nil — 返回 interface 的函数容易出错".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "Go 中 interface nil 和 concrete type nil 是不同的:\n\
                       var x *MyType = nil; var i interface{} = x; // i != nil!\n\
                       检查所有返回 interface 类型的函数，确保返回的 nil 是真正的 nil。".into(),
                examples: vec![
                    "Bad: func get() io.Writer { var f *os.File; return f }  // non-nil".into(),
                    "Good: func get() io.Writer { return nil }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "defer-in-loop".into(),
            description: "循环中的 defer — 在函数结束时才执行，不是迭代结束".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "在 for 循环中使用 defer 会导致资源在函数退出时才释放。\
                       需要立即清理时用匿名函数包装。".into(),
                examples: vec![
                    "Bad: for _, f := range files { f,_ := os.Open(f); defer f.Close() }".into(),
                    "Good: for _, f := range files { func() { f,_ := os.Open(f); defer f.Close() }() }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "error-not-checked".into(),
            description: "未检查的 error 返回值 — 不允许用 _ 忽略 error".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有返回 error 的函数调用必须检查错误。\
                       不允许使用 _ 忽略 error (除非有明确的注释说明原因)。".into(),
                examples: vec![
                    "Bad: data, _ := ioutil.ReadAll(r)".into(),
                    "Good: data, err := ioutil.ReadAll(r); if err != nil { return err }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "context-propagation".into(),
            description: "Context 传播 — 所有 I/O 操作都必须传递 context".into(),
            category: "performance".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有网络调用、数据库查询、gRPC/HTTP 调用必须传递 context。\
                       不允许在请求处理器中使用 context.Background()。".into(),
                examples: vec![
                    "Bad: db.Query(\"SELECT ...\")".into(),
                    "Good: db.QueryContext(ctx, \"SELECT ...\")".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

#### 5.4.3 `src/rules/python.rs`

```rust
//! Python 规则包 — 从 OCR 的 Python 审查经验 + ruff/bandit 规则

pub fn python_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "type-safety".into(),
            description: "缺少类型注解 — 公共 API 必须有完整类型注解".into(),
            category: "maintainability".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有公共函数和方法必须有类型注解 (参数 + 返回值)。\
                       使用 typing: Optional, Union, TypeVar, Protocol。\
                       不通过 mypy --strict 的代码不应合并。".into(),
                examples: vec![
                    "Bad: def process(data):".into(),
                    "Good: def process(data: list[dict[str, Any]]) -> Result[User, AppError]:".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "async-await-correctness".into(),
            description: "协程泄漏、事件循环阻塞、不正确的并发".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查异步代码:\n\
                       1. 是否在 async def 中用了 time.sleep() 而不是 asyncio.sleep()?\n\
                       2. Task/协程是否被正确 await 或 gather?\n\
                       3. 是否有未关闭的 aiohttp session?\n\
                       4. 异步上下文管理器是否正确处理了异常?".into(),
                examples: vec![
                    "Bad: time.sleep(1)  # blocks event loop in async function".into(),
                    "Good: await asyncio.sleep(1)".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "sql-injection".into(),
            description: "SQL 注入 — 禁止字符串拼接 SQL".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有 SQL 查询必须使用参数化查询。\
                       禁止 f-string 或 .format() 拼接 SQL。".into(),
                examples: vec![
                    "Bad: cursor.execute(f\"SELECT * FROM users WHERE id = {uid}\")".into(),
                    "Good: cursor.execute(\"SELECT * FROM users WHERE id = %s\", (uid,))".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "broad-except".into(),
            description: "过度宽泛的 except — 不能裸捕获所有异常".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "不要使用裸 except: 或 except Exception:。\
                       至少让 KeyboardInterrupt 和 SystemExit 传播。".into(),
                examples: vec![
                    "Bad: except: pass".into(),
                    "Good: except ValueError as e: logger.warning(...); raise".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

#### 5.4.4 `src/rules/typescript.rs`

```rust
//! TypeScript 规则包

pub fn typescript_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "null-safety".into(),
            description: "空值安全 — 避免 undefined is not an object 崩溃".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查 null/undefined 安全:\n\
                       1. 使用 optional chaining (?.) 和 nullish coalescing (??)\n\
                       2. 不要在类型断言 (as / !) 中绕过 null 检查\n\
                       3. Promise.catch 必须处理 rejection\n\
                       4. API response 的 shape 必须在访问前验证".into(),
                examples: vec![
                    "Bad: const name = response.data.user.name;  // may crash".into(),
                    "Good: const name = response?.data?.user?.name ?? 'Unknown';".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "xss-injection".into(),
            description: "XSS 注入 — 用户输入不应直接渲染为 HTML".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查所有渲染用户输入的地方:\n\
                       1. dangerouslySetInnerHTML 必须先消毒\n\
                       2. innerHTML / document.write 必须消毒输入\n\
                       3. URL 参数必须 encodeURIComponent".into(),
                examples: vec![
                    "Bad: <div dangerouslySetInnerHTML={{__html: userInput}} />".into(),
                    "Good: <div>{sanitize(userInput)}</div>".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "promise-unhandled".into(),
            description: "未处理的 Promise — 必须有 .catch 或 try/catch".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "每个 Promise 必须处理失败:\n\
                       1. async/await → try/catch 包裹\n\
                       2. .then() → 必须有 .catch()\n\
                       3. Promise.all: 一个失败时其他仍在运行\n\
                       4. 不要在 forEach 中用 async (不会等待)".into(),
                examples: vec![
                    "Bad: items.forEach(async (item) => { await process(item); });".into(),
                    "Good: await Promise.all(items.map(item => process(item)));".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

### 5.5 文件: `src/verifiers/deterministic/lint.rs` — 多语言 Lint 分发

```rust
//! Lint Verifier — 按语言分发的确定性 Lint 检查
//!
//! 每种语言有各自的原生 lint 工具:
//!   Rust → clippy
//!   Go   → golangci-lint
//!   Python → ruff
//!   TS/JS → eslint
//!   Java → checkstyle
//!   C++ → clang-tidy
//!   Proto → buf lint
//!   SQL → sqlfluff

use async_trait::async_trait;
use onto_assurance_runtime::verification::{
    DeterministicVerifierPort, DeterministicVerificationRequest,
    DeterministicVerifierError, VerifierCapability,
};
use onto_assurance_types::finding::{FindingCandidate, Severity, EvidenceLocationClaim};
use onto_assurance_types::ids::{FindingId, RuleId, VerifierId};
use onto_assurance_types::hash::ContentHash;

pub struct MultiLanguageLintVerifier {
    capability: VerifierCapability,
    verifier_id: VerifierId,
}

impl MultiLanguageLintVerifier {
    pub fn new() -> Self {
        let vid = VerifierId::new();
        Self {
            verifier_id: vid,
            capability: VerifierCapability {
                verifier_id: vid,
                verifier_version: "1.0.0".into(),
                supported_target_kinds: vec![
                    onto_assurance_types::verification_target::TargetKind::SourceFile,
                ],
                supported_languages: vec![
                    "rust".into(), "go".into(), "python".into(),
                    "typescript".into(), "javascript".into(),
                    "java".into(), "cpp".into(), "protobuf".into(), "sql".into(),
                ],
                max_target_size_bytes: 10 * 1024 * 1024, // 10 MiB
                supports_batching: true,
                max_batch_size: 200,
                cost_per_token_microcents: 0, // 确定性检查，无 LLM token 成本
            },
        }
    }

    /// 按语言路由到正确的 lint 命令和参数。
    fn lint_command_for(&self, lang: &str) -> Option<(&str, Vec<&str>)> {
        match lang {
            "rust" => Some(("cargo", vec!["clippy", "--all-targets", "--all-features",
                "--", "-D", "warnings"])),
            "go" => Some(("golangci-lint", vec!["run", "--out-format", "json"])),
            "python" => Some(("ruff", vec!["check", "--output-format", "json"])),
            "typescript" | "javascript" => Some(("npx", vec!["eslint", "--format", "json"])),
            "java" => Some(("mvn", vec!["checkstyle:check", "-q"])),
            "cpp" | "c" => Some(("clang-tidy", vec![])), // targets passed separately
            "protobuf" => Some(("buf", vec!["lint", "--format", "json"])),
            "sql" => Some(("sqlfluff", vec!["lint", "--format", "json"])),
            _ => None,
        }
    }
}

#[async_trait]
impl DeterministicVerifierPort for MultiLanguageLintVerifier {
    fn capability(&self) -> &VerifierCapability {
        &self.capability
    }

    async fn verify(
        &self,
        request: DeterministicVerificationRequest,
    ) -> Result<Vec<FindingCandidate>, DeterministicVerifierError> {
        // 1. 按语言分组 targets
        // 2. 对每个语言组执行 lint 命令
        // 3. 解析每个语言的输出 → FindingCandidate
        // 4. 合并所有结果
        let mut all_findings = Vec::new();

        // 检测语言 (从第一个 target 的扩展名推断)
        // 生产代码中应从 LanguageRegistry 获取
        let lang = detect_language_from_targets(&request.unit.target_refs);

        if let Some((cmd, args)) = self.lint_command_for(&lang) {
            let mut command = std::process::Command::new(cmd);
            command.args(&args);
            command.current_dir(&request.workspace_path);

            let output = tokio::time::timeout(
                std::time::Duration::from_millis(request.timeout_ms),
                tokio::process::Command::from(command).output(),
            ).await
                .map_err(|_| DeterministicVerifierError::Timeout(request.timeout_ms))?
                .map_err(|e| DeterministicVerifierError::ExecutionFailed(e.to_string()))?;

            let findings = parse_lint_output(
                &lang,
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            );
            all_findings.extend(findings);
        }

        Ok(all_findings)
    }
}

fn detect_language_from_targets(targets: &[String]) -> String {
    // 按多数语言判定
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for t in targets {
        let ext = std::path::Path::new(t).extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = match ext {
            "rs" => "rust",
            "go" => "go",
            "py" | "pyi" => "python",
            "ts" | "tsx" | "js" | "jsx" | "mjs" => "typescript",
            "java" => "java",
            "c" | "h" | "cpp" | "hpp" | "cc" => "cpp",
            "proto" => "protobuf",
            "sql" => "sql",
            _ => "unknown",
        };
        *counts.entry(lang).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, c)| *c)
        .map(|(l, _)| l.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_lint_output(lang: &str, stdout: &str, _stderr: &str) -> Vec<FindingCandidate> {
    match lang {
        "rust" => parse_clippy_output(stdout),
        "go" => parse_golangci_lint(stdout),
        "python" => parse_ruff_output(stdout),
        "typescript" | "javascript" => parse_eslint_output(stdout),
        _ => vec![],
    }
}

fn parse_clippy_output(stdout: &str) -> Vec<FindingCandidate> {
    let mut findings = Vec::new();
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("");
            if reason != "compiler-message" {
                continue;
            }
            let msg = &v["message"];
            let spans = msg["spans"].as_array();
            let primary = spans.and_then(|s| s.first());
            let file = primary
                .and_then(|s| s["file_name"].as_str()).unwrap_or("");
            let line_num = primary
                .and_then(|s| s["line_start"].as_u64()).unwrap_or(0) as u32;
            let text = msg["message"].as_str().unwrap_or("");

            findings.push(FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(),
                    start_line: Some(line_num),
                    end_line: Some(line_num),
                    code_snippet: None,
                    symbol_name: None,
                    byte_offset: None,
                }),
                title: format!("clippy: {}", text),
                description: text.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0, // 确定性检查 = 100% 置信
                suggested_fix: None,
                rationale: None,
                verifier_metadata: v.clone(),
            });
        }
    }
    findings
}

fn parse_golangci_lint(stdout: &str) -> Vec<FindingCandidate> {
    // golangci-lint JSON 格式: [{ "Pos": { "Filename": "...", "Line": N }, "Text": "..." }]
    let mut findings = Vec::new();
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(stdout) {
        for item in arr {
            let file = item["Pos"]["Filename"].as_str().unwrap_or("");
            let line = item["Pos"]["Line"].as_u64().unwrap_or(0) as u32;
            let text = item["Text"].as_str().unwrap_or("");
            findings.push(FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(), start_line: Some(line), end_line: Some(line),
                    code_snippet: None, symbol_name: None, byte_offset: None,
                }),
                title: format!("golangci-lint: {}", text),
                description: text.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0,
                suggested_fix: None,
                rationale: None,
                verifier_metadata: item.clone(),
            });
        }
    }
    findings
}

fn parse_ruff_output(stdout: &str) -> Vec<FindingCandidate> {
    // ruff JSON 格式: [{ "filename": "...", "location": { "row": N }, "message": "..." }]
    serde_json::from_str::<Vec<serde_json::Value>>(stdout)
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let file = item["filename"].as_str().unwrap_or("");
            let line = item["location"]["row"].as_u64().unwrap_or(0) as u32;
            let code = item["code"].as_str().unwrap_or("");
            let msg = item["message"].as_str().unwrap_or("");
            FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(), start_line: Some(line), end_line: Some(line),
                    code_snippet: None, symbol_name: None, byte_offset: None,
                }),
                title: format!("ruff({}): {}", code, msg),
                description: msg.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0,
                suggested_fix: None,
                rationale: None,
                verifier_metadata: item.clone(),
            }
        })
        .collect()
}

fn parse_eslint_output(stdout: &str) -> Vec<FindingCandidate> {
    // eslint JSON 格式: [{ "filePath": "...", "messages": [{ "line": N, "message": "..." }] }]
    serde_json::from_str::<Vec<serde_json::Value>>(stdout)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|file_entry| {
            let file = file_entry["filePath"].as_str().unwrap_or("").to_string();
            let messages = file_entry["messages"].as_array().cloned().unwrap_or_default();
            messages.into_iter().map(move |msg| {
                let line = msg["line"].as_u64().unwrap_or(0) as u32;
                let text = msg["message"].as_str().unwrap_or("");
                let rule = msg["ruleId"].as_str().unwrap_or("");
                FindingCandidate {
                    finding_id: FindingId::new(),
                    verifier_id: VerifierId::new(),
                    rule_id: RuleId::new(),
                    target_ref: file.clone(),
                    target_content_hash: ContentHash::new(""),
                    claimed_location: Some(EvidenceLocationClaim {
                        path: file.clone(), start_line: Some(line), end_line: Some(line),
                        code_snippet: None, symbol_name: None, byte_offset: None,
                    }),
                    title: format!("eslint({}): {}", rule, text),
                    description: text.into(),
                    severity: Severity::Medium,
                    verifier_confidence: 1.0,
                    suggested_fix: None,
                    rationale: None,
                    verifier_metadata: msg.clone(),
                }
            }).collect::<Vec<_>>()
        })
        .collect()
}
```

### 5.6 文件: `src/verifiers/semantic/prompt_builder.rs` — 按语言构建提示

这个模块生成**语言特定的 System Prompt**。Rust 的 prompt 和 Python 的 prompt 不同，因为每种语言有独特的陷阱。

```rust
//! 按语言构建 SemanticVerifier 的 System Prompt。
//!
//! 核心原则: 每种语言有独特的陷阱和习惯用法 — prompt 必须反映这一点。
//! 通用 prompt 会导致遗漏语言特定的关键问题。

use onto_assurance_types::rule_binding::Rule;

/// 为指定语言构建语义审查的 System Prompt。
pub fn build_system_prompt(language: &str, rules: &[Rule]) -> String {
    let lang_guidance = match language {
        "rust" => RUST_SYSTEM,
        "go" => GO_SYSTEM,
        "python" => PYTHON_SYSTEM,
        "typescript" | "javascript" => TS_SYSTEM,
        "java" => JAVA_SYSTEM,
        "cpp" | "c" => CPP_SYSTEM,
        _ => "",
    };

    let rules_text = rules.iter()
        .filter_map(|r| match &r.content {
            onto_assurance_types::rule_binding::RuleContent::NaturalLanguage { text, .. } => {
                Some(format!("- {}: {}", r.name, text))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("\
You are reviewing {lang} code. Report all defects found.

{guidance}

## Rules

{rules}

## Output Format

For each finding, output a JSON object:
```json
{{
  \"path\": \"file path\",
  \"title\": \"one-line summary\",
  \"description\": \"detailed explanation\",
  \"severity\": \"critical|high|medium|low|info\",
  \"start_line\": line_number,
  \"end_line\": line_number,
  \"existing_code\": \"the problematic code snippet\"
}}
```

Do NOT output a summary or declare success/failure.",
        lang = language, guidance = lang_guidance, rules = rules_text)
}

// ══════════════════════════════════════════════════════════════════
// 语言特定 System Prompt 模板
// ══════════════════════════════════════════════════════════════════

const RUST_SYSTEM: &str = "\
## Rust-Specific Checks

- **Ownership**: Unnecessary clones, borrow checker workarounds hiding bugs,
  lifetime annotations too short or too long
- **Unsafe**: Every unsafe block MUST have a SAFETY comment. Verify invariants.
- **Error Handling**: Errors propagated (not swallowed). ? not used in main()
  without context. Appropriate error types.
- **Concurrency**: Missing Send/Sync bounds. Deadlocks (Mutex lock ordering).
  Data races (unsafe + raw pointers).
- **Macros**: Hygiene — no accidental capture of external names. No silent panics.
- **Patterns**: Exhaustive match arms. No wildcard matches hiding new enum variants.";

const GO_SYSTEM: &str = "\
## Go-Specific Checks

- **Goroutines**: Every goroutine needs a clear exit path (context, channel close,
  select+default). Goroutine leaks in HTTP handlers.
- **Nil Safety**: Interface nil vs concrete nil. Type assertions handle nil.
  Map access checks zero-value correctly.
- **Error Handling**: Every error checked. No _ for errors without comment.
  Error wrapping with %w for unwrap at upper levels.
- **Defer**: Defer in loops = dangerous. Resource cleanup order.
  Named return values with defer — verify intentional modification.
- **Concurrency**: Channel ops — no perpetual blocking. sync.Mutex unlocked
  on all paths (including panic). sync.WaitGroup Add before goroutine launch.
- **Context**: All I/O must pass context. No context.Background() in handlers.";

const PYTHON_SYSTEM: &str = "\
## Python-Specific Checks

- **Type Safety**: Type annotations on all public functions. Optional vs None.
  Union types correct. Protocol conformance validated.
- **Async/Await**: time.sleep() in async = blocks event loop. Unawaited coroutines.
  Missing aiohttp session close.
- **Security**: SQL injection via f-strings. Command injection in subprocess.
  Pickle deserialization of untrusted data. Hardcoded secrets.
- **Exception Handling**: No bare except: or except Exception: without re-raising.
  Custom exception hierarchy — inheritance correct.
- **Performance**: Generator vs list for large data. Unnecessary comprehensions
  building intermediate lists. __del__ with circular references.";

const TS_SYSTEM: &str = "\
## TypeScript-Specific Checks

- **Null Safety**: Optional chaining (?.) and nullish coalescing (??).
  Non-null assertions (!) that may fail. API response shape validation.
- **XSS**: dangerouslySetInnerHTML, innerHTML, document.write — must sanitize.
  URL parameters encoded (encodeURIComponent).
- **Promises**: Unhandled rejections. forEach with async (doesn't await).
  Promise.all — one failure leaves others running.
- **Type System**: any usage must be justified. Type assertions (as) verified.
  Generic type parameter constraints checked.
- **React**: Missing keys in lists. useEffect cleanup. State update after unmount.
  Unnecessary re-renders.
- **Security**: eval(), new Function(). postMessage without origin check.
  localStorage for sensitive data.";

const JAVA_SYSTEM: &str = "\
## Java-Specific Checks

- **Concurrency**: synchronized lock ordering. volatile vs Atomic*.
  ThreadLocal cleanup. ExecutorService shutdown on all paths.
- **JPA/Hibernate**: N+1 queries. Lazy loading outside transaction.
  Missing @Transactional. Entity equals/hashCode inconsistent.
- **Security**: SQL injection (JPQL concatenation). XXE in XML parsers.
  Path traversal. Unsafe deserialization.
- **Resources**: try-with-resources for AutoCloseable. Unclosed streams.
  Connection pool leaks.
- **Null**: @Nullable/@NonNull consistency. Optional.orElse(null) = anti-pattern.";

const CPP_SYSTEM: &str = "\
## C/C++-Specific Checks

- **Memory**: new/delete pairing, use-after-free, double free.
  Memory leaks in exception paths.
- **UB**: Signed overflow, null pointer deref, out-of-bounds access,
  strict aliasing violations.
- **RAII**: Destructors noexcept. Rule of 5/3/0 violations.
  Resource acquisition in constructors — no bare pointers owning memory.
- **Concurrency**: Data races. Missing atomic. Mutex deadlock.
  Condition variable spurious wakeup handled.";
```

### 5.7 文件: `src/verifiers/registry.rs` — 多语言 Verifier 注册表

```rust
//! VerifierRegistry — 根据 LanguageProfile 选择合适的 Verifier 集合

use std::collections::HashMap;
use onto_assurance_types::language_profile::{LanguageProfile, LanguageRegistry};
use onto_assurance_types::ids::VerifierId;
use onto_assurance_runtime::verification::{SemanticVerifierPort, DeterministicVerifierPort};

/// 管理所有已注册的 Verifier，按语言路由。
pub struct VerifierRegistry {
    semantic: HashMap<VerifierId, Box<dyn SemanticVerifierPort>>,
    deterministic: HashMap<VerifierId, Box<dyn DeterministicVerifierPort>>,
}

impl VerifierRegistry {
    pub fn new() -> Self {
        Self { semantic: HashMap::new(), deterministic: HashMap::new() }
    }

    pub fn register_semantic(&mut self, v: Box<dyn SemanticVerifierPort>) {
        self.semantic.insert(v.capability().verifier_id, v);
    }

    pub fn register_deterministic(&mut self, v: Box<dyn DeterministicVerifierPort>) {
        self.deterministic.insert(v.capability().verifier_id, v);
    }

    /// 为给定语言选择所有匹配的语义 Verifier。
    pub fn semantic_for(&self, profile: &LanguageProfile) -> Vec<&dyn SemanticVerifierPort> {
        profile.verifiers.semantic.iter()
            .filter_map(|id| self.semantic.get(id).map(|v| v.as_ref()))
            .collect()
    }

    /// 为给定语言选择所有适用的确定性 Verifier。
    pub fn deterministic_for(&self, profile: &LanguageProfile) -> Vec<&dyn DeterministicVerifierPort> {
        let ids = [
            &profile.verifiers.build, &profile.verifiers.test,
            &profile.verifiers.lint, &profile.verifiers.format_check,
            &profile.verifiers.sast, &profile.verifiers.dependency_audit,
        ];
        let mut verifiers: Vec<&dyn DeterministicVerifierPort> = ids.iter()
            .filter_map(|id| id.as_ref())
            .filter_map(|id| self.deterministic.get(id).map(|v| v.as_ref()))
            .collect();
        for (_, id) in &profile.verifiers.extras {
            if let Some(v) = self.deterministic.get(id) {
                verifiers.push(v.as_ref());
            }
        }
        verifiers
    }
}
```

### 5.8 文件: `src/scope/polyglot.rs` — 多语言仓库处理

```rust
//! Polyglot — 多语言仓库的按语言分片和预算分配

use onto_assurance_types::verification_target::VerificationTarget;
use onto_assurance_types::language_profile::LanguageRegistry;
use std::collections::HashMap;

/// 按语言对 VerificationTarget 分组。
///
/// 一个仓库可能同时有 Rust + TypeScript + Protobuf。
/// 每种语言需要不同的 Verifier 集合。
pub fn group_by_language(
    targets: &[VerificationTarget],
    registry: &LanguageRegistry,
) -> HashMap<String, Vec<VerificationTarget>> {
    let mut groups: HashMap<String, Vec<VerificationTarget>> = HashMap::new();
    for target in targets {
        let lang = target.language.as_deref().unwrap_or("unknown");
        groups.entry(lang.to_string()).or_default().push(target.clone());
    }
    tracing::info!(
        total = targets.len(),
        languages = ?groups.keys().collect::<Vec<_>>(),
        "polyglot scope grouped by language"
    );
    groups
}

/// 计算多语言仓库的 token 估算 — 用于预算分配。
///
/// 不同语言的 token/byte 比例不同:
///   Go:     ~1.5 tokens/byte (长关键字，显式错误处理)
///   Rust:   ~1.3 tokens/byte
///   Python: ~1.1 tokens/byte (简洁语法)
///   TS:     ~1.4 tokens/byte
pub fn estimate_tokens_by_language(
    groups: &HashMap<String, Vec<VerificationTarget>>,
    registry: &LanguageRegistry,
) -> HashMap<String, u64> {
    let mut estimates = HashMap::new();
    for (lang, targets) in groups {
        let tokens_per_byte = registry.get(lang)
            .map(|p| p.tokens_per_byte)
            .unwrap_or(1.5);
        let total_bytes: u64 = targets.iter().map(|t| t.size_bytes).sum();
        estimates.insert(lang.clone(), (total_bytes as f64 * tokens_per_byte) as u64);
    }
    estimates
}

/// 按语言比例分配语义 Verifier 的调用预算。
///
/// 例如: 总预算 50 次语义调用，Rust 30个文件，Go 20个文件
///   → Rust: min(30, 50*0.6) = 30, Go: min(20, 50*0.4) = 20
pub fn allocate_semantic_budget(
    groups: &HashMap<String, Vec<VerificationTarget>>,
    total_budget: u32,
) -> HashMap<String, u32> {
    let total_targets: usize = groups.values().map(|v| v.len()).sum();
    if total_targets == 0 { return HashMap::new(); }

    groups.iter().map(|(lang, targets)| {
        let proportion = targets.len() as f64 / total_targets as f64;
        let allocation = (total_budget as f64 * proportion).ceil() as u32;
        // 不能超过该语言的目标数 (语义审查按文件)
        (lang.clone(), allocation.min(targets.len() as u32))
    }).collect()
}
```

---

### 5.9 文件: `src/domain_pack.rs` — 通用 DomainPack trait

```rust
//! DomainPack trait — 所有领域包的统一接口。
//!
//! 每个领域实现此 trait，提供:
//!   1. 领域特定的 TargetKind 支持
//!   2. 领域特定的 ScopeResolver (怎么枚举目标)
//!   3. 领域特定的 LocationResolver (怎么定位发现)
//!   4. 领域特定的 RulePack
//!   5. 领域特定的 Verifier 集合
//!
//! onto-code-pack 是第一个实现。后续可以有:
//!   onto-document-pack, onto-data-pack, onto-workflow-pack,
//!   onto-config-pack, onto-ops-pack, onto-simulation-pack

use async_trait::async_trait;
use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use onto_assurance_types::scope_manifest::ScopeManifest;
use onto_assurance_types::rule_binding::RulePack;
use onto_assurance_types::finding::EvidenceLocation;
use onto_assurance_types::ids::VerifierId;
use onto_assurance_runtime::verification::{SemanticVerifierPort, DeterministicVerifierPort};

/// 一个领域包的完整能力声明。
#[derive(Debug, Clone)]
pub struct DomainCapability {
    /// 领域名称: "code", "document", "data", "workflow", "config", "ops", "simulation"
    pub domain_name: String,
    /// 领域版本
    pub domain_version: String,
    /// 支持的 TargetKind
    pub supported_target_kinds: Vec<TargetKind>,
    /// 领域特定的 MIME 类型 / 文件扩展名 / 资源类型
    pub supported_formats: Vec<String>,
}

/// DomainPack — 统一领域包接口。
///
/// 每个领域包负责: "针对此类目标，如何枚举范围、如何定位发现、用哪些规则和 Verifier"。
/// VerificationCoordinator 通过此 trait 在不了解领域细节的情况下编排验证。
#[async_trait]
pub trait DomainPack: Send + Sync {
    /// 返回领域能力声明。
    fn capability(&self) -> &DomainCapability;

    /// 枚举此领域的目标范围。
    /// 例如: 代码领域 = git diff → 文件列表
    ///        文档领域 = doc tree → 章节列表
    ///        数据领域 = schema scan → 表/字段列表
    async fn enumerate_targets(
        &self,
        scope_spec: &ScopeSpecification,
    ) -> Result<Vec<VerificationTarget>, DomainError>;

    /// 返回此领域的规则包。
    fn rule_packs(&self) -> Vec<RulePack>;

    /// 返回此领域的语义 Verifier。
    fn semantic_verifiers(&self) -> Vec<Box<dyn SemanticVerifierPort>>;

    /// 返回此领域的确定性 Verifier。
    fn deterministic_verifiers(&self) -> Vec<Box<dyn DeterministicVerifierPort>>;

    /// 领域特定的位置解析。
    /// 输入 FindingCandidate + target content → 输出 EvidenceLocation (或 Unresolved)
    fn resolve_location(
        &self,
        claim: &onto_assurance_types::finding::EvidenceLocationClaim,
        target_content: &[u8],
    ) -> Option<EvidenceLocation>;
}

/// 通用的范围规约 — 不同领域用不同字段。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScopeSpecification {
    /// 范围模式: "diff" | "scan" | "paths" | "query" | "checkpoint"
    pub mode: String,

    /// diff 参数 (mode=diff)
    pub diff_from: Option<String>,
    pub diff_to: Option<String>,
    pub diff_commit: Option<String>,

    /// 路径参数 (mode=paths)
    pub paths: Vec<String>,

    /// 查询参数 (mode=query) — 领域特定
    /// 数据领域: SQL WHERE clause
    /// 工作流领域: workflow_id
    /// 文档领域: glob pattern
    pub query: Option<String>,

    /// 排除模式
    pub exclude_patterns: Vec<String>,

    /// 领域特定的额外参数
    pub extra: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("scope error: {0}")]
    Scope(String),
    #[error("unsupported target kind: {0:?}")]
    UnsupportedTargetKind(TargetKind),
    #[error("internal: {0}")]
    Internal(String),
}
```

### 5.10 领域示例: OntoDocumentPack — 文档验证

文档验证的独特之处:
- 目标不是文件，而是**文档中的章节/段落/条款**
- 位置不是行号，而是**页码 + 段落 + 文本指纹**
- 规则关注: 一致性、完整性、术语正确性、与代码的一致性

```rust
//! OntoDocumentPack — 文档领域的 Verification Fabric 实现
//!
//! 支持的目标类型:
//!   - Markdown / reStructuredText / AsciiDoc 文档
//!   - API 文档 (OpenAPI / GraphQL schema)
//!   - 合同 / 规范文档
//!   - README / CHANGELOG / 架构决策记录 (ADR)

use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use onto_assurance_types::finding::{EvidenceLocation, EvidenceLocationClaim};
use onto_assurance_types::ids::VerificationTargetId;
use onto_assurance_types::hash::ContentHash;

/// 文档段落 — 文档领域的最小验证单元。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentSection {
    pub section_id: String,          // e.g. "3.2.1" or "security-considerations"
    pub heading: String,             // e.g. "## Security Considerations"
    pub content: String,             // full text of this section
    pub paragraph_index: usize,      // 在文档中的段落序号
    pub parent_section_id: Option<String>,
    pub children: Vec<String>,       // 子 section_ids
    pub content_hash: ContentHash,
    pub doc_path: String,
}

/// 文档验证规则示例。
pub fn document_rule_pack() -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "readme-completeness".into(),
            description: "README 必须包含安装、使用、配置、贡献四个必要部分".into(),
            category: "documentation".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查 README 是否包含:\n\
                       1. Installation 章节 (如何安装)\n\
                       2. Usage 章节 (基本使用示例)\n\
                       3. Configuration 章节 (关键配置项)\n\
                       4. Contributing 章节 (或引用 CONTRIBUTING.md)".into(),
                examples: vec!["Missing 'Installation' section in README".into()],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "api-doc-code-consistency".into(),
            description: "API 文档中的接口签名必须与实际代码一致".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "对比 OpenAPI spec 中的 endpoint 与实际 handler 代码:\n\
                       1. 路径和方法是否匹配\n\
                       2. 请求/响应 schema 字段是否匹配\n\
                       3. 状态码定义是否匹配\n\
                       4. 标记为 deprecated 的字段在代码中是否也已标记".into(),
                examples: vec![
                    "OpenAPI says GET /users/:id returns 'email' field, but handler only returns 'id' and 'name'".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "contract-terminology-consistency".into(),
            description: "合同/规范文档中的术语必须与代码中的命名一致".into(),
            category: "consistency".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查规范文档中定义的术语是否在代码中一致使用。\
                       例如: 文档说 'TransactionCoordinator'，代码中是 'TxCoordinator' = 不一致".into(),
                examples: vec![],
            },
        },
    ];
    RulePack { /* ... */ }
}

/// 文档位置 — 替换 EvidenceLocation::SourceCode
pub fn resolve_document_location(
    claim: &EvidenceLocationClaim,
    sections: &[DocumentSection],
) -> Option<EvidenceLocation> {
    let snippet = claim.code_snippet.as_deref().unwrap_or("");
    if snippet.is_empty() { return None; }

    // 在所有 section 的 content 中模糊搜索 snippet
    for section in sections {
        if section.content.contains(snippet) {
            // 计算在该 section 中的段落偏移
            let offset = section.content[..section.content.find(snippet)?].lines().count();
            return Some(EvidenceLocation::Document {
                doc_path: section.doc_path.clone(),
                section_id: section.section_id.clone(),
                paragraph_hash: section.content_hash.clone(),
            });
        }
    }
    None
}
```

### 5.11 领域示例: OntoWorkflowPack — 工作流验证

工作流验证的独特之处:
- 目标是 **Workflow Node / Transition / State Machine**
- 位置是 `workflow_id + node_id + transition_id`
- 规则关注: 可达性、死锁、未处理状态、超时、幂等性

```rust
//! OntoWorkflowPack — 工作流领域的 Verification Fabric
//!
//! 验证 Temporal / OntoFlow / state machine 定义的正确性。
//! 与 onto-temporal-adapter 紧密集成。

/// 工作流节点 — 工作流领域的最小验证单元。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowNode {
    pub workflow_id: String,
    pub node_id: String,
    pub node_type: NodeType,
    pub transitions: Vec<WorkflowTransition>,
    pub timeout_ms: Option<u64>,
    pub retry_policy: Option<RetryPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Activity,
    Decision,
    Timer,
    Signal,
    SubWorkflow,
    HumanTask,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowTransition {
    pub to_node_id: String,
    pub condition: Option<String>,
    pub error_transition: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub non_retryable_errors: Vec<String>,
}

/// 工作流验证规则。
pub fn workflow_rule_pack() -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "unreachable-nodes".into(),
            description: "工作流中存在不可达的节点".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查工作流图中是否存在没有任何入边的节点 (除了起始节点)。\
                       从起始节点做 BFS/DFS 遍历，所有节点必须可达。".into(),
                examples: vec!["Node 'approval_3' has no incoming transitions".into()],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "missing-error-transition".into(),
            description: "Activity 节点没有错误转换 — 异常会导致工作流卡死".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "每个 Activity 节点必须有至少一条 error_transition=true 的边，\
                       或显式声明 catch-all 错误处理。否则 Activity 失败时工作流会卡死。".into(),
                examples: vec![
                    "Node 'send_email' has no error transition. On failure, workflow stalls.".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "infinite-retry-loop".into(),
            description: "RetryPolicy 可能导致无限重试".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查 RetryPolicy:\n\
                       1. max_attempts 是否设置 (无限制 = 潜在无限重试)\n\
                       2. non_retryable_errors 是否合理 (某些错误不应重试)\n\
                       3. 错误转换是否在 max_attempts 耗尽后可达".into(),
                examples: vec![],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "idempotency-key-missing".into(),
            description: "有副作用的 Activity 缺少幂等键".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有具有外部副作用 (写数据库、发邮件、调用 API) 的 Activity \
                       必须有 idempotency_key。Temporal 的 at-most-once 语义依赖于此。".into(),
                examples: vec![],
            },
        },
    ];
    RulePack { /* ... */ }
}

/// 工作流可达性分析 — 确定性算法。
pub fn find_unreachable_nodes(nodes: &[WorkflowNode], start_node_id: &str) -> Vec<String> {
    let adjacency: std::collections::HashMap<&str, Vec<&str>> = nodes.iter()
        .map(|n| (n.node_id.as_str(), n.transitions.iter().map(|t| t.to_node_id.as_str()).collect()))
        .collect();

    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![start_node_id];
    while let Some(id) = stack.pop() {
        if visited.insert(id) {
            if let Some(nexts) = adjacency.get(id) {
                stack.extend(nexts);
            }
        }
    }

    nodes.iter()
        .map(|n| n.node_id.clone())
        .filter(|id| !visited.contains(id.as_str()))
        .collect()
}

/// 工作流位置。
pub fn resolve_workflow_location(
    claim: &EvidenceLocationClaim,
    nodes: &[WorkflowNode],
) -> Option<EvidenceLocation> {
    let node_id = claim.symbol_name.as_deref()?;
    let node = nodes.iter().find(|n| n.node_id == node_id)?;
    Some(EvidenceLocation::Workflow {
        workflow_id: node.workflow_id.clone(),
        node_id: node.node_id.clone(),
        transition_id: claim.code_snippet.clone(),
    })
}
```

### 5.12 领域示例: OntoDataPack — 数据验证

数据验证的独特之处:
- 目标是 **表 / 字段 / 分区 / 记录**
- 位置是 `schema.table.column + primary_key`
- 规则关注: 数据质量、约束、隐私合规、新鲜度

```rust
//! OntoDataPack — 数据领域的 Verification Fabric
//!
//! 验证数据集的质量、完整性、合规性。

/// 数据目标 — 数据库中的一个表/视图/物化视图。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataTarget {
    pub schema_name: String,
    pub table_name: String,
    pub column_name: Option<String>,
    pub data_type: Option<String>,       // PostgreSQL type: "integer", "text", "timestamptz"
    pub nullable: bool,
    pub primary_key: bool,
    pub has_index: bool,
    pub row_count_estimate: Option<u64>,
    pub size_bytes_estimate: Option<u64>,
}

/// 数据验证规则。
pub fn data_rule_pack() -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "pii-unencrypted".into(),
            description: "PII (个人身份信息) 列未加密或未脱敏".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "扫描所有列名和注释，检测可能包含 PII 的列:\n\
                       email, phone, ssn, passport, credit_card, address, name, dob, ip_address\n\
                       检查:\n\
                       1. 是否启用了列级加密?\n\
                       2. 是否有数据脱敏策略?\n\
                       3. 是否记录在数据分类清单中?".into(),
                examples: vec![
                    "Column 'users.email' contains PII but has no encryption policy".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "missing-unique-constraint".into(),
            description: "应该有唯一约束但没有的列".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检测名为 *email*, *username*, *slug*, *uuid*, *external_id* 的列 \
                       是否有 UNIQUE 约束或唯一索引。如果没有，标记为潜在数据完整性问题。".into(),
                examples: vec![],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "stale-partition".into(),
            description: "分区表有过期分区 — 可能导致查询性能下降".into(),
            category: "performance".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查分区表:\n\
                       1. 是否有超过 N 天未写入的分区?\n\
                       2. 是否有未来日期的分区 (配置错误)?\n\
                       3. 分区键的分布是否均匀?".into(),
                examples: vec![],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "schema-drift".into(),
            description: "相同表名在不同环境中 schema 不一致".into(),
            category: "consistency".into(),
            content: RuleContent::NaturalLanguage {
                text: "比较 dev/staging/prod 中同名的表:\n\
                       1. 列数和列名是否一致?\n\
                       2. 数据类型是否一致?\n\
                       3. 约束 (NOT NULL, DEFAULT) 是否一致?".into(),
                examples: vec![],
            },
        },
    ];
    RulePack { /* ... */ }
}

/// 数据位置。
pub fn resolve_data_location(
    claim: &EvidenceLocationClaim,
) -> Option<EvidenceLocation> {
    let schema = claim.symbol_name.as_deref()?;
    let parts: Vec<&str> = schema.split('.').collect();
    if parts.len() < 2 { return None; }
    Some(EvidenceLocation::Dataset {
        schema_name: parts[0].to_string(),
        table_name: parts[1].to_string(),
        column_name: parts.get(2).map(|s| s.to_string()),
        row_key: claim.code_snippet.clone(),
    })
}
```

### 5.13 领域注册: 让 VerificationCoordinator 发现所有 DomainPack

```rust
//! onto-code-pack/src/lib.rs 补充 — DomainPack 注册

use crate::domain_pack::DomainPack;
use std::sync::Arc;

/// 构建包含所有已实现领域的 DomainPack 列表。
///
/// 当前已实现:
///   - CodePack (Rust, Go, Python, TypeScript, Java, C++, Proto, SQL)
///   - DocumentPack (Markdown, OpenAPI, ADR)
///   - WorkflowPack (Temporal/OntoFlow)
///   - DataPack (PostgreSQL schema)
///
/// 未来:
///   - ConfigPack (YAML/TOML/JSON config validation)
///   - OpsPack (infrastructure as code, Terraform, K8s manifests)
///   - SimulationPack (Gazebo/Isaac Sim scene validation)
pub fn all_domain_packs() -> Vec<Arc<dyn DomainPack>> {
    vec![
        Arc::new(CodePack::new()),
        Arc::new(DocumentPack::new()),
        Arc::new(WorkflowPack::new()),
        Arc::new(DataPack::new()),
    ]
}

/// VerificationCoordinator 使用示例:
///
/// ```ignore
/// let packs = all_domain_packs();
/// let coordinator = VerificationCoordinator::new(packs, ...);
/// let results = coordinator.run(ScopeSpecification {
///     mode: "diff".into(),
///     paths: vec![],
///     query: None,
///     ..Default::default()
/// }).await?;
/// // results 包含: 代码缺陷 + 文档问题 + 工作流问题 + 数据问题
/// ```
```

### 5.14 跨领域交叉验证

Verification Fabric 最强大的能力: **跨领域交叉验证**。

```rust
//! 跨领域交叉验证 — 一个领域的发现触发另一个领域的验证

/// 跨领域交叉验证规则。
pub struct CrossDomainRule {
    /// 触发条件: 当 domain_a 中满足此条件时
    pub trigger_domain: String,
    pub trigger_finding_category: String,

    /// 触发的验证: 在 domain_b 中执行此规则
    pub target_domain: String,
    pub target_rule_name: String,
}

/// 内置的跨领域规则。
pub fn builtin_cross_domain_rules() -> Vec<CrossDomainRule> {
    vec![
        // 代码修改了 API handler → 验证 OpenAPI 文档是否需要更新
        CrossDomainRule {
            trigger_domain: "code".into(),
            trigger_finding_category: "api-change".into(),
            target_domain: "document".into(),
            target_rule_name: "api-doc-code-consistency".into(),
        },
        // 代码修改了数据库 schema → 验证数据合规性
        CrossDomainRule {
            trigger_domain: "code".into(),
            trigger_finding_category: "schema-change".into(),
            target_domain: "data".into(),
            target_rule_name: "pii-unencrypted".into(),
        },
        // 工作流修改了 Activity → 验证重试策略和幂等键
        CrossDomainRule {
            trigger_domain: "code".into(),
            trigger_finding_category: "activity-change".into(),
            target_domain: "workflow".into(),
            target_rule_name: "idempotency-key-missing".into(),
        },
        // 数据 schema 变更 → 验证相关的工作流节点是否需要更新
        CrossDomainRule {
            trigger_domain: "data".into(),
            trigger_finding_category: "schema-change".into(),
            target_domain: "workflow".into(),
            target_rule_name: "missing-error-transition".into(),
        },
        // API 文档声称的功能 → 验证是否有测试覆盖
        CrossDomainRule {
            trigger_domain: "document".into(),
            trigger_finding_category: "api-doc".into(),
            target_domain: "code".into(),
            target_rule_name: "missing-test".into(),
        },
    ]
}
```

---

## 6. Go→Rust 算法移植指南



## 6. Go→Rust 算法移植指南






以下是对每个关键算法的移植映射。

### 6.1 VerificationTarget::fingerprint

**源文件:** `internal/session/persist.go` + `internal/agent/agent.go`

**Go 源码:**
```go
// 隐式: 文件指纹 = hash(file_path + diff_content + rule_hash)
// 在 agent.go 中通过 sha256 计算
```

**Rust 移植:**
```rust
impl VerificationTarget {
    pub fn fingerprint(&self, domain: &HashDomain) -> ContentHash {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(domain.as_bytes());
        hasher.update(self.target_kind.to_string().as_bytes());
        hasher.update(self.target_ref.as_bytes());
        hasher.update(self.content_hash.as_bytes());
        if let Some(lang) = &self.language {
            hasher.update(lang.as_bytes());
        }
        ContentHash::from_bytes(&hasher.finalize())
    }
}
```

**关键差异:**
- Go: `sha256.Sum256([]byte)` → Rust: `sha2::Sha256::digest()`
- Go: `fmt.Sprintf("%x", hash)` → Rust: `hex::encode(hash)`
- 两者 SHA-256 输出一致，可互操作

---

### 6.2 ScopeManifest::validate_invariants

**源文件:** OCR 无显式验证（Go 中靠 `FileFilter` 保证覆盖）

**Go 概念:**
```go
// internal/config/rules/system_rules.go
// Resolver.Resolve(path) → rule
// 不变量: 每个文件要么匹配规则，要么使用 default_rule
// 但没有 ScopeManifest 级别的显式验证
```

**Rust 移植（OCR 概念 + OntoAssure 不变量）:**
```rust
impl ScopeManifest {
    pub fn validate_invariants(
        &self,
        all_candidate_refs: &[String],
    ) -> Result<(), String> {
        let included_refs: std::collections::HashSet<_> =
            self.included_targets.iter().map(|t| &t.target_ref).collect();
        let excluded_refs: std::collections::HashSet<_> =
            self.excluded_targets.iter().map(|t| &t.target_ref).collect();

        // S-1: 每个候选目标必须在 included 或 excluded 中
        for r in all_candidate_refs {
            if !included_refs.contains(r) && !excluded_refs.contains(r) {
                return Err(format!("target {r} silently omitted from scope"));
            }
        }

        // S-2: included ∩ excluded = ∅
        for r in &included_refs {
            if excluded_refs.contains(r) {
                return Err(format!("target {r} is both included and excluded"));
            }
        }

        // S-3: 每个 excluded 有 reason（类型系统已保证）
        Ok(())
    }
}
```

---

### 6.3 glob_match — doublestar 移植

**源文件:** Go `github.com/bmatcuk/doublestar/v4`

**Go 用法:**
```go
import "github.com/bmatcuk/doublestar/v4"
doublestar.Match(pattern, path)
```

**Rust 移植:** 使用 `globset` crate（功能等价）或手动实现核心语法:

```toml
# Cargo.toml
[dependencies]
globset = "0.4"
```

```rust
use globset::{Glob, GlobSetBuilder};

fn glob_match(pattern: &str, path: &str) -> bool {
    // 简单路径: 直接用 globset
    if let Ok(g) = Glob::new(pattern) {
        return g.compile_matcher().is_match(path);
    }
    false
}
```

**关键差异:**
- Go: `doublestar.Match("**/*.rs", "src/main.rs")` → Rust: `Glob::new("**/*.rs")?.compile_matcher().is_match("src/main.rs")`
- 语法兼容: `**`, `*`, `?`, `[abc]` 在两者中语义一致
- `{a,b}` 大括号展开: Rust globset 不原生支持，需要先手动展开

---

### 6.4 规则匹配逻辑移植

**源文件:** `internal/config/rules/system_rules.go`

**Go 源码:**
```go
func (r *SystemRule) Resolve(path string) string {
    for _, pr := range r.PathRules {
        if match, _ := doublestar.Match(pr.Pattern, path); match {
            return pr.Rule
        }
    }
    return r.DefaultRule
}
```

**Rust 移植:**
```rust
impl RulePack {
    pub fn resolve(&self, target_ref: &str, target_kind: TargetKind) -> Vec<&Rule> {
        // Step 1: 仅适用 target_kind
        if !self.applicable_target_kinds.contains(&target_kind) {
            return vec![];
        }

        // Step 2: 按优先级匹配
        // 移植 OCR 的 PathRules 顺序匹配逻辑
        self.rules.iter().filter(|rule| {
            match &rule.content {
                RuleContent::NaturalLanguage { .. } => true, // NL 规则适用所有
                RuleContent::Pattern { pattern, .. } => glob_match(pattern, target_ref),
                RuleContent::Policy { .. } => true, // 策略由 PolicyEngine 进一步过滤
            }
        }).collect()
    }
}
```

---

### 6.5 去重 + Criterion 映射

**源文件:** OCR `internal/scan/agent.go` + `internal/tool/comment_collector.go`

**Go 概念:**
```go
// CommentCollector 收集评论，Scan Agent 在批处理中做去重
// 无显式 Criterion 映射（OCR 无此概念）
```

**Rust 移植:**
```rust
// 去重: 移植 OCR CommentCollector 的去重逻辑
pub fn deduplicate(resolved: &[ResolvedFinding]) -> Vec<CorroboratedFinding> {
    let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
    // ... (已在 §3.4 中实现)
}

// Criterion 映射: OntoAssure 特有
pub fn map_to_criteria(
    findings: &[CorroboratedFinding],
    criteria: &[Criterion],
) -> Vec<CorroboratedFinding> {
    findings.iter().map(|f| {
        let mapped: Vec<CriterionId> = criteria.iter()
            .filter(|c| {
                // 规则: finding.severity >= criterion 要求的最低严重度
                // 或 finding.category 匹配 criterion 的域
                f.resolved.candidate.severity >= c.min_severity()
                || c.applicable_categories.contains(&f.resolved.candidate.rule_id.to_string())
            })
            .map(|c| c.criterion_id)
            .collect();

        CorroboratedFinding {
            mapped_criteria: mapped,
            ..f.clone()
        }
    }).collect()
}
```

---

### 6.6 DiffScopeProvider::enumerate() — 完整移植

**Go 源码** (`internal/diff/git.go`):
```go
func (p *Provider) Diff(ctx context.Context) ([]model.Diff, error) {
    // 1. 执行 git diff 命令
    // 2. 解析 unified diff 输出
    // 3. 为每个文件构建 model.Diff
    // 4. 过滤 ignoreDirs + 二进制
}
```

**Rust 移植:**
```rust
impl DiffScopeProvider {
    pub fn enumerate(&self) -> Result<Vec<VerificationTarget>, DiffError> {
        // 1. 执行 git diff --name-only
        let output = std::process::Command::new("git")
            .arg("-C").arg(&self.repo_dir)
            .arg("diff").arg("--name-only")
            .arg("--diff-filter=ACMR")  // Added, Copied, Modified, Renamed
            .output()
            .map_err(|e| DiffError::Git(e.to_string()))?;

        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        // 2. 过滤: ignoreDirs, 二进制（移植自 OCR providerDirIgnoreDirs）
        let ignored = [
            ".idea/", ".vscode/", ".svn/", ".git/",
            "vendor/", "node_modules/", "target/",
            ".happypack/", ".cachefile/", "_packages/",
            "rpm/", "pkgs/",
        ];

        let filtered: Vec<String> = files
            .into_iter()
            .filter(|f| !ignored.iter().any(|i| f.starts_with(i)))
            .collect();

        // 3. 为每个文件读取内容 + 计算 hash + 检测语言
        let targets: Vec<VerificationTarget> = filtered
            .into_iter()
            .filter_map(|path| {
                let full_path = std::path::Path::new(&self.repo_dir).join(&path);
                let content = std::fs::read(&full_path).ok()?;

                // 二进制检测（移植自 OCR scan provider 的 sniff 逻辑）
                if is_binary(&content) {
                    return None;
                }

                let size = content.len() as u64;
                let hash = ContentHash::from_bytes(&sha2::Sha256::digest(&content));
                let language = detect_language(&path);

                Some(VerificationTarget {
                    target_id: VerificationTargetId::new(),
                    target_kind: TargetKind::SourceFile,
                    target_ref: path,
                    content_hash: hash,
                    size_bytes: size,
                    language,
                    risk_level: onto_assurance_types::enums::RiskLevel::Medium,
                    metadata: serde_json::Value::Null,
                })
            })
            .collect();

        Ok(targets)
    }
}

/// 二进制检测 — 移植自 OCR scan.Provider 的 binarySniffWindow
fn is_binary(data: &[u8]) -> bool {
    let window = &data[..data.len().min(8000)];
    window.contains(&0x00)
}

/// 简单语言检测 — 移植自 OCR 的允许列表逻辑
fn detect_language(path: &str) -> Option<String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => Some("rust".to_string()),
        "go" => Some("go".to_string()),
        "py" => Some("python".to_string()),
        "ts" | "tsx" => Some("typescript".to_string()),
        "js" | "jsx" => Some("javascript".to_string()),
        "java" => Some("java".to_string()),
        "c" | "h" => Some("c".to_string()),
        "cpp" | "hpp" | "cc" | "hh" => Some("cpp".to_string()),
        "proto" => Some("protobuf".to_string()),
        "sql" => Some("sql".to_string()),
        "yaml" | "yml" => Some("yaml".to_string()),
        "json" => Some("json".to_string()),
        "toml" => Some("toml".to_string()),
        "md" | "mdx" => Some("markdown".to_string()),
        _ => None,
    }
}
```

---

### 6.7 位置解析完整移植

**Go 源码** (`internal/diff/resolver.go`，238行):
```
ResolveLineNumbers()    → §3.3 resolve_locations()
resolveFromHunk()       → §3.3 resolve_from_diff_hunks()
resolveFromFileContent()→ §3.3 resolve_from_full_content()
matchConsecutive()      → §3.3 match_consecutive()
normalizeLine()         → §3.3 normalize_line()
```

**移植状态:** 已在 §3.3 中完整移植。所有纯函数算法保持相同逻辑。

**移植检查清单:**
- [x] `ResolveLineNumbers` → `resolve_locations`（批量接口）
- [x] `resolveFromHunk` → `resolve_from_diff_hunks`（hunk 级匹配）
- [x] `resolveFromFileContent` → `resolve_from_full_content`（文件级扫描）
- [x] `matchConsecutive` → `match_consecutive`（滑动窗口匹配）
- [x] `normalizeLine` → `normalize_line`（trim + strip diff markers）
- [x] `splitAndNormalize` → `normalize_and_split`（代码分块）
- [ ] `extractSideLines` → hunk line extraction（需移植 unified diff parser）

**待移植:**
- OCR `ParseHunks()` — unified diff parser
  - Go: `internal/diff/hunk.go` + `internal/diff/parser.go`
  - Rust: 用 `similar` crate 或移植解析器

---

## 7. VerifierProvider 接口 (定义于 onto-assurance-runtime)

> **归属说明:** `SemanticVerifierPort` 和 `DeterministicVerifierPort` 定义在 `onto-assurance-runtime`，
> 而不是在 IronClaw 内部。这保证控制权不逆转：OntoAssure 定义接口，IronClaw 实现接口。
> 具体实现 `IronClawSemanticVerifier` 放在 `onto-ironclaw-adapter`。

```rust
//! onto-assurance-runtime/src/verification/ports.rs
//! Verifier Port traits — 所有 Verifier 的统一接口
//!
//! 关键约束:
//!   - 定义在 onto-assurance-runtime，保证 OntoAssure 是接口所有者
//!   - Verifier 只能返回 FindingCandidate，不能宣告 Success/Failure
//!   - Verifier 不能自己决定范围或规则
//!   - Verifier 的输出是 untrusted — 必须经过 LocationResolver + EvidenceBuilder

use async_trait::async_trait;
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_plan::VerificationUnit;
use onto_assurance_types::ids::VerifierId;

/// Verifier 能力描述符。
#[derive(Debug, Clone)]
pub struct VerifierCapability {
    pub verifier_id: VerifierId,
    pub verifier_version: String,
    /// 支持的 target_kind
    pub supported_target_kinds: Vec<onto_assurance_types::verification_target::TargetKind>,
    /// 支持的语言（SourceFile 时有效）
    pub supported_languages: Vec<String>,
    /// 最大单目标大小（字节）
    pub max_target_size_bytes: u64,
    /// 是否支持批量（多个 target 在一次调用中处理）
    pub supports_batching: bool,
    /// 最大批量大小
    pub max_batch_size: usize,
    /// 估算的每 token 成本（microcents）
    pub cost_per_token_microcents: u64,
}

/// 语义 Verifier 的输入——范围、规则、预算都是确定的。
#[derive(Debug, Clone)]
pub struct SemanticVerificationRequest {
    pub unit: VerificationUnit,
    /// 完整的 target 内容（不依赖 Verifier 自己去读文件）
    pub target_contents: Vec<TargetContentWithHash>,
    /// 适用的规则（完整内容，不只是引用）
    pub applicable_rules: Vec<onto_assurance_types::rule_binding::Rule>,
    /// 补充上下文（如 contract 的背景信息）
    pub context: Option<String>,
}

/// 确定性 Verifier 的输入（Build/Test/Lint）。
#[derive(Debug, Clone)]
pub struct DeterministicVerificationRequest {
    pub unit: VerificationUnit,
    pub workspace_path: String,
    pub command: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TargetContentWithHash {
    pub target_ref: String,
    pub content: String,
    pub content_hash: onto_assurance_types::hash::ContentHash,
}

/// 语义 Verifier 的错误类型。
#[derive(Debug, thiserror::Error)]
pub enum SemanticVerifierError {
    #[error("verifier unavailable: {0}")]
    Unavailable(String),
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("target too large: {0} bytes", .0)]
    TargetTooLarge(u64),
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("verifier internal error: {0}")]
    Internal(String),
}

/// 确定性 Verifier 的错误类型。
#[derive(Debug, thiserror::Error)]
pub enum DeterministicVerifierError {
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("timeout after {0}ms", .0)]
    Timeout(u64),
    #[error("parse output failed: {0}")]
    ParseError(String),
}

/// 语义 Verifier: 对给定 VerificationUnit 做深度语义分析。
///
/// 这是 IronClaw 的新位置——不再是全能审查器。
#[async_trait]
pub trait SemanticVerifierPort: Send + Sync {
    /// 返回能力描述符
    fn capability(&self) -> &VerifierCapability;

    /// 验证——输入已确定范围、规则、预算
    async fn verify(
        &self,
        request: SemanticVerificationRequest,
    ) -> Result<Vec<FindingCandidate>, SemanticVerifierError>;
}

/// 确定性 Verifier: Build / Test / Lint / SAST。
#[async_trait]
pub trait DeterministicVerifierPort: Send + Sync {
    fn capability(&self) -> &VerifierCapability;

    async fn verify(
        &self,
        request: DeterministicVerificationRequest,
    ) -> Result<Vec<FindingCandidate>, DeterministicVerifierError>;
}
```

---

## 8. 线路协议 — 与 Go OntoFlow 的契约

### 8.1 现有协议（不变）

OntoFlow 通过 Temporal ActivityTask 调用 Rust Worker，现有协议:

```
Go OntoFlow                         Rust Worker
══════════                          ══════════
LoopInvocationRequest  ──────────▶  OntoLoopWorker::run_or_resume_to_terminal()
LoopTerminalEnvelope    ◀──────────  (返回 terminal state)
```

### 8.2 新增协议: VerificationInvocation

Verification Fabric 作为新的 Activity 类型暴露给 Go OntoFlow:

```rust
//! VerificationInvocation — Go OntoFlow 调用 Verification Fabric 的协议

use serde::{Deserialize, Serialize};

/// Go → Rust: 请求执行一次验证运行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationInvocationRequest {
    // ── 继承自 LoopInvocationRequest 的通用字段 ──
    pub schema_version: u32,
    pub flow_id: String,
    pub work_item_id: String,
    pub invocation_id: String,         // 对应 loop_id
    pub execution_generation: u64,
    pub idempotency_key: String,
    pub request_binding_hash: String,

    // ── 验证专用字段 ──
    /// 合约引用
    pub contract_ref: String,
    /// 规则包引用列表
    pub rule_pack_refs: Vec<String>,
    /// 范围模式: "diff" | "scan" | "paths"
    pub scope_mode: String,
    /// diff 参数 (scope_mode=diff 时有效)
    pub diff_from: Option<String>,
    pub diff_to: Option<String>,
    pub diff_commit: Option<String>,
    /// 路径参数 (scope_mode=paths 时有效)
    pub target_paths: Vec<String>,
    /// 排除模式
    pub exclude_patterns: Vec<String>,
    /// 预算
    pub budget: Option<SerializableBudget>,
    /// 恢复: 之前的 session_id
    pub resume_from_session_id: Option<String>,
}

/// Rust → Go: 验证运行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationInvocationResponse {
    // ── 通用字段 ──
    pub schema_version: u32,
    pub invocation_id: String,
    pub request_binding_hash: String,

    // ── 结果字段 ──
    /// 验证是否完整（是否因预算耗尽而截断）
    pub complete: bool,
    /// 原因
    pub completion_reason: String,

    // ── 统计 ──
    pub total_targets: u32,
    pub included_targets: u32,
    pub excluded_targets: u32,
    pub units_executed: u32,
    pub findings_total: u32,
    pub findings_corroborated: u32,
    pub evidence_records_produced: u32,

    // ── 预算 ──
    pub budget_status: String,
    pub tokens_used: u64,

    // ── 引用（不是完整内容）──
    pub evidence_bundle_ref: Option<String>,
    pub session_id: String,
    pub outcome_binding_hash: String,
}

/// 可序列化的预算配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableBudget {
    pub max_total_tokens: Option<u64>,
    pub max_total_cost_cents: Option<u64>,
    pub max_wall_time_seconds: Option<u64>,
    pub max_parallel_units: u32,
    pub max_semantic_invocations: u32,
    pub max_target_size_bytes: u64,
}
```

### 8.3 协议不变量

```
1. Rust Worker 返回的 outcome_binding_hash
   = hash(request_binding_hash || 所有输出字段)
   → Go 必须验证

2. complete=false 时:
   - 绝不能产生 PASS 判决
   - 必须返回 budget_status 说明原因

3. evidence_bundle_ref 指向持久化的 EvidenceBundle
   → Go 可通过 AuthorityProjectionPort 验证

4. session_id 可用于后续请求的 resume_from_session_id
   → 允许分多次 Activity 完成大型验证
```

---

## 9. IronClaw 适配 — 从全能代理到双重角色

### 9.1 IronClaw 依然保持完整本体

IronClaw 不是被降级成"代码审查插件"。它保留两个角色：

```
IronClaw Runtime
├── Lane A: Agent Execution Lane
│   │
│   │  被 OntoLoop 调用，完成一个 Attempt 中的开放式执行
│   │
│   │  输入: objective (自然语言), context, tools, budget
│   │  输出: Candidate Checkpoint (产物 + 自我评估 + 退出原因)
│   │
│   │  调用方向: OntoLoop → IronClaw Lane A
│   │
│   └── Lane B: Semantic Verification Lane
│
│      被 OntoAssure Verification Fabric 受限调用
│
│      输入: VerificationUnit + RuleBindings + BoundedContext + BudgetGrant
│      输出: FindingCandidate[] (仅此)
│
│      不拥有: 范围选择权、规则解释权、Evidence 宣告权、Success 宣告权
│
│      调用方向: OntoAssure → IronClaw Lane B
```

### 9.2 现状 (反模式)

```
IronClaw review-pr / review-crate (Claude Code)
  → 自己决定范围 (Step 3: Read every changed file)
  → 自己解释规则 (Step 4: Deep review across 6 lenses)
  → 自己定位行号 (Step 6: Post comments at specific line)
  → 自己宣告质量 (Step 5: Present findings as table with severity)

问题: 运动员、裁判、仲裁机构是同一个系统
```

### 9.3 目标 (正确架构)

```
执行阶段 (Lane A):
  OntoLoop → IronClaw Agent Loop → Candidate Checkpoint

验证阶段 (Lane B):
  OntoAssure
  → Scope/Rules/Plan (确定性)
  → 调度 IronClaw Lane B + BuildVerifier + TestVerifier + LintVerifier
  → FindingCandidate[] (来自所有 Verifier)
  → LocationResolver → Dedup → EvidenceBuilder (确定性)
  → Reducer → SessionDecision → Settlement (确定性)
```

### 9.4 适配层实现

**接口定义:** `onto-assurance-runtime/src/verification/ports.rs` (已在 §7)
**接口实现:** `onto-ironclaw-adapter/src/semantic_verifier_adapter.rs`

```rust
// onto-code-pack/src/verifiers/ironclaw_semantic.rs

use async_trait::async_trait;
use onto_assurance_runtime::verification::SemanticVerifierPort;
use onto_assurance_runtime::verification::{
    SemanticVerificationRequest, SemanticVerifierError,
    VerifierCapability,
};
use onto_assurance_types::finding::{FindingCandidate, Severity};
use onto_assurance_types::ids::{FindingId, RuleId, VerifierId};

/// IronClaw 适配器 — 将 IronClaw 作为 SemanticVerifierPort 的实现。
pub struct IronClawSemanticVerifier {
    capability: VerifierCapability,
    /// IronClaw agent 的入口（现有的 Claude Code 或 Rust agent loop）
    ironclaw_runtime: Arc<dyn IronClawInvoker>,
}

/// 调用 IronClaw 的底层接口。
#[async_trait]
trait IronClawInvoker: Send + Sync {
    async fn invoke(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[ToolDef],
        budget_tokens: u64,
    ) -> Result<Vec<FindingCandidate>, String>;
}

#[async_trait]
impl SemanticVerifierPort for IronClawSemanticVerifier {
    fn capability(&self) -> &VerifierCapability {
        &self.capability
    }

    async fn verify(
        &self,
        request: SemanticVerificationRequest,
    ) -> Result<Vec<FindingCandidate>, SemanticVerifierError> {
        // ── 构建 prompt ──
        // 注意: 这里不包含范围选择逻辑——那些已在 Verification Fabric 中完成。
        // 只包含: 目标内容 + 规则 + 输出 Schema 约束

        let system_prompt = build_system_prompt(&request.applicable_rules);
        let user_prompt = build_user_prompt(&request.unit, &request.target_contents);

        // ── 调用 IronClaw ──
        let raw_findings = self.ironclaw_runtime.invoke(
            &system_prompt,
            &user_prompt,
            &build_tool_defs(),
            request.unit.budget_grant.max_tokens.unwrap_or(100_000),
        ).await.map_err(|e| SemanticVerifierError::Internal(e))?;

        // ── 后处理 ──
        // 1. 移除 IronClaw 可能自行添加的"总体结论"
        // 2. 确保每个 Finding 有足够的元数据
        // 3. 标记 verifier_id
        let findings = raw_findings.into_iter().map(|mut f| {
            f.verifier_id = self.capability.verifier_id;
            f
        }).collect();

        Ok(findings)
    }
}

fn build_system_prompt(rules: &[onto_assurance_types::rule_binding::Rule]) -> String {
    let mut prompt = String::from(
        "You are a code reviewer. Your task is to find defects in the provided code.\n\n"
    );
    prompt.push_str("## Rules to apply:\n\n");
    for rule in rules {
        match &rule.content {
            onto_assurance_types::rule_binding::RuleContent::NaturalLanguage { text, .. } => {
                prompt.push_str(&format!("- {}: {}\n", rule.name, text));
            }
            _ => {}
        }
    }
    prompt.push_str("\n## Output format\n\n");
    prompt.push_str(
        "For each finding, output a JSON object with: path, title, description, severity, start_line, end_line, existing_code.\n"
    );
    prompt.push_str("Do NOT output a summary. Do NOT declare whether the task succeeded.\n");
    prompt
}

fn build_user_prompt(
    unit: &onto_assurance_types::verification_plan::VerificationUnit,
    targets: &[TargetContentWithHash],
) -> String {
    let mut prompt = format!(
        "Review the following {} file(s):\n\n",
        targets.len()
    );
    for t in targets {
        prompt.push_str(&format!(
            "### File: {}\n```\n{}\n```\n\n",
            t.target_ref, t.content
        ));
    }
    prompt.push_str("Please report all findings as JSON objects.\n");
    prompt
}
```

### 9.4 IronClaw 能力描述符

```rust
impl IronClawSemanticVerifier {
    pub fn new(ironclaw_runtime: Arc<dyn IronClawInvoker>) -> Self {
        Self {
            capability: VerifierCapability {
                verifier_id: VerifierId::new(),
                verifier_version: env!("CARGO_PKG_VERSION").to_string(),
                supported_target_kinds: vec![
                    onto_assurance_types::verification_target::TargetKind::SourceFile,
                ],
                supported_languages: vec![
                    "rust".into(), "go".into(), "python".into(),
                    "typescript".into(), "javascript".into(),
                    "java".into(), "c".into(), "cpp".into(),
                ],
                max_target_size_bytes: 200 * 1024, // ~200 KB per target
                supports_batching: true,
                max_batch_size: 5,
                cost_per_token_microcents: 1500, // Anthropic Claude cost
            },
            ironclaw_runtime,
        }
    }
}
```

---

## 10. 迁移路径

### Phase 1: Types + Pure Functions (不依赖运行时)

**时间:** 2-3 weeks
**范围:** onto-assurance-types + onto-assurance-core

```
新增文件:
  onto-assurance-types/src/
    verification_target.rs
    scope_manifest.rs
    verification_plan.rs
    verification_session.rs
    finding.rs
    verification_budget.rs
    rule_binding.rs

  onto-assurance-core/src/verification/
    mod.rs
    scope_coverage.rs
    rule_binding.rs
    location_binding.rs
    finding_normalization.rs
```

**验证:** 所有纯函数有单元测试（≥80% 覆盖率，参考 OCR 的 coverage 标准）

---

### Phase 2: Trait Definitions (接口)

**时间:** 1 week
**范围:** onto-assurance-runtime + onto-code-pack

```
  onto-assurance-runtime/src/verification/
    planner.rs      (PlanGenerator trait)
    scheduler.rs    (UnitScheduler trait)
    session.rs      (SessionManager trait)

  onto-code-pack/src/
    lib.rs          (VerifierCapability, trait 重导出)
```

**验证:** trait 编译通过 + 有 mock 实现用于测试

---

### Phase 3: Runtime Implementation (编排器)

**时间:** 3-4 weeks

```
  onto-assurance-runtime/src/verification/
    coordinator.rs  (VerificationCoordinator)
    budget.rs       (BudgetTracker)
    locator.rs      (LocationResolverService)
    evidence_builder.rs

  onto-code-pack/src/
    scope/diff.rs           (DiffScopeProvider)
    scope/repository_scan.rs
    rules/rust.rs, go.rs, ...
```

**验证:** 集成测试: diff → targets → plan → mock verifier → findings → evidence

---

### Phase 4: IronClaw Adaption

**时间:** 2-3 weeks

```
  onto-code-pack/src/verifiers/
    ironclaw_semantic.rs

  IronClaw 侧修改:
    - 从 review-pr / review-crate 中提取"纯语义分析"能力
    - 移除范围选择、规则解释、成功宣告逻辑
    - 实现 SemanticVerifierPort trait
```

**验证:** 对比: 同一个 PR，IronClaw 独立审查 vs IronClaw 通过 Verification Fabric 审查

---

### Phase 5: OntoFlow Integration

**时间:** 2 weeks

```
新增:
  temporal/.../verification_activity.go   (Go Activity 实现)
  onto-temporal-adapter/src/verification_handler.rs (Rust 侧 handler)

协议:
  VerificationInvocationRequest → Response (见 §8)
```

**验证:** Temporal workflow: 触发 PR → OntoFlow → Verification Activity → 结果展示

---

### Phase 6: OpenCodeReview 退役计划

**时间:** Phase 5 完成后

```
OpenCodeReview (Go)
  ├── 算法已移植 → 删除对应 Go 模块
  ├── 接口已泛化 → 不再需要 Go 运行时
  └── CI workflows → 替换为 OntoFlow Verification Activity
```

OCR 剩余的独特价值（npm 包分发、多 CI 平台适配）可保留为社区维护，核心算法不再独立演进。

---

## 附录 A: 文件清单

```
新增/修改文件: ~35 个 Rust 源文件

┌── onto-assurance-types (新增 8 个模块)
│   ├── verification_target.rs
│   ├── scope_manifest.rs
│   ├── verification_plan.rs
│   ├── verification_session.rs
│   ├── finding.rs
│   ├── verification_budget.rs
│   ├── rule_binding.rs
│   └── language_profile.rs
│
├── onto-assurance-core (新增 1 个子模块 verification/)
│   └── verification/
│       ├── mod.rs
│       ├── scope_coverage.rs
│       ├── rule_binding.rs
│       ├── location_binding.rs
│       └── finding_normalization.rs
│
├── onto-assurance-runtime (新增 1 个子模块 verification/)
│   ├── verification/
│   │   ├── mod.rs
│   │   ├── ports.rs          ★ SemanticVerifierPort & DeterministicVerifierPort 定义
│   │   ├── coordinator.rs
│   │   ├── planner.rs
│   │   ├── scheduler.rs
│   │   ├── session.rs
│   │   ├── budget.rs
│   │   ├── locator.rs
│   │   └── evidence_builder.rs
│   └── ...
│
├── onto-code-pack (新增 4 个子模块 — 代码领域实现)
│   ├── domain_pack.rs        ★ DomainPack trait
│   ├── scope/
│   │   ├── mod.rs
│   │   ├── diff.rs           ← OCR internal/diff/git.go
│   │   ├── repository_scan.rs← OCR internal/scan/provider.go
│   │   └── polyglot.rs
│   ├── rules/
│   │   ├── mod.rs
│   │   ├── rust.rs           ← IronClaw review-discipline.md + clippy
│   │   ├── go.rs             ← OCR Go 审查经验 + golangci-lint
│   │   ├── python.rs         ← OCR Python 审查经验 + ruff
│   │   ├── typescript.rs
│   │   ├── java.rs
│   │   ├── cpp.rs
│   │   ├── protobuf.rs
│   │   └── generic.rs
│   ├── verifiers/
│   │   ├── deterministic/
│   │   │   ├── mod.rs
│   │   │   ├── build.rs
│   │   │   ├── test.rs
│   │   │   ├── lint.rs        ★ MultiLanguageLintVerifier
│   │   │   ├── format_check.rs
│   │   │   ├── sast.rs
│   │   │   └── dependency_audit.rs
│   │   ├── semantic/
│   │   │   ├── mod.rs
│   │   │   ├── prompt_builder.rs  ★ 按语言构建 System Prompt
│   │   │   └── generic_llm.rs
│   │   └── registry.rs
│   ├── location/
│   │   ├── mod.rs
│   │   ├── source_code.rs    ← OCR internal/diff/resolver.go (行号定位)
│   │   ├── ast.rs
│   │   └── symbol.rs
│   └── session/
│       └── jsonl.rs          ← OCR internal/session/persist.go
│
├── onto-ironclaw-adapter (修改 1 个文件)
│   └── semantic_verifier_adapter.rs  ★ implements SemanticVerifierPort
│                                      (不是 defines — 接口定义在 runtime)
│
└── onto-temporal-adapter (新增 1 个 handler)
    └── verification_handler.rs

★★★ 关键归属: ★★★

SemanticVerifierPort 定义在: onto-assurance-runtime/src/verification/ports.rs
SemanticVerifierPort 实现在: onto-ironclaw-adapter/src/semantic_verifier_adapter.rs

不在 IronClaw 内部定义接口 — 否则控制权倒置
```

移植自 OCR 的 Go 源码:

| OCR Go 源文件 | Rust 目标文件 | 归属 crate |
|--------------|--------------|-----------|
| `internal/diff/git.go` | `onto-code-pack/src/scope/diff.rs` | onto-code-pack |
| `internal/diff/resolver.go` | `onto-assurance-core/src/verification/location_binding.rs` | onto-assurance-core |
| `internal/diff/hunk.go` | `onto-code-pack/src/scope/hunk.rs` | onto-code-pack |
| `internal/diff/parser.go` | `onto-code-pack/src/scope/parser.rs` | onto-code-pack |
| `internal/config/rules/*.go` | `onto-assurance-core/src/verification/rule_binding.rs` | onto-assurance-core |
| `internal/session/persist.go` | `onto-code-pack/src/session/jsonl.rs` | onto-code-pack |
| `internal/session/resume.go` | `onto-assurance-runtime/src/verification/session.rs` | onto-assurance-runtime |
| `internal/scan/provider.go` | `onto-code-pack/src/scope/repository_scan.rs` | onto-code-pack |

不移植:

| OCR Go 组件 | 原因 |
|------------|------|
| `internal/agent/agent.go` | 编排器已在 VerificationCoordinator 中重写 |
| `internal/llm/` | 使用 IronClaw 现有的 LLM 基础设施 |
| `internal/llmloop/` | 使用 OntoLoop 现有的循环基础设施 |
| `internal/tool/` | VerifierProvider trait 体系替换 |
| `internal/viewer/` | 未来单独评估 |
| `cmd/opencodereview/` | CLI 由 ontoctl 或 Go OntoFlow 替换 |

---

## 附录 B: 与现有 OntoAssure 的交叉引用

| 新增组件 | 复用已有组件 |
|---------|------------|
| VerificationTarget.content_hash | `onto_assurance_types::hash::ContentHash` (已有) |
| ScopeManifest.manifest_hash | `onto_assurance_types::hash::ContentHash` (已有) |
| VerificationUnit.unit_fingerprint | `onto_assurance_core::canonical::compute_hash` (已有) |
| EvidenceLocation | 可序列化为 `EvidenceRecord.payload` (已有) |
| FindingCandidate → EvidenceRecord | `onto_assurance_core::evidence_chain::EvidenceChain` (已有) |
| CorroboratedFinding.mapped_criteria | `onto_assurance_types::ids::CriterionId` (已有) |
| VerificationSession.bindings | `onto_assurance_types::ids::*` (已有) |
| BudgetStatus | `onto_assurance_types::enums::BudgetOutcome` (已有，可扩展) |
| VerificationCoordinator.run() | `TransactionCoordinator.execute_attempt()` (已有，参考设计) |
| Session 恢复 | `onto_loop::checkpoint` (已有，参考指纹匹配逻辑) |
| Plan 生成 (分组策略) | `onto_loop::progress` (已有，参考 ProgressComparison) |

---

## 附录 C: 关键设计决策记录

1. **不做 Go 运行时:** 所有 OCR 有价值的部分都是确定性算法，不需要 Go 运行时的并发/LLM 基础设施

2. **Verifier 不宣告 Success:** 只有 OntoAssure Decision Authority 可以宣告 TaskOutcome。Verifier 只产出 FindingCandidate

3. **FindingCandidate 不可信:** 必须经过 LocationResolution + Dedup + 可能 Cross-verification → 才能升级为 EvidenceRecord

4. **预算耗尽 ≠ Success:** `verify()` 返回 `BudgetExhausted` 时，调用者绝不能将其视为"通过验证"

5. **Session 恢复需指纹验证:** 5 个绑定（contract_hash, rule_pack_hash, verifier_version_hash, target_content_hash, unit_fingerprint）任一不匹配 → 结果失效

6. **规则包是版本化实体:** RulePack.pack_hash 确保"使用不同版本的规则审查"意味着不同结果

7. **协议保持引用语义:** Go OntoFlow 只发送引用（contract_ref, rule_pack_refs），不发送完整内容

8. **位置解析是纯函数:** 不依赖 LLM、不依赖网络、不依赖运行时状态。相同输入 → 相同输出（利于 replay/reproduce）

9. **★★★ 接口定义归属:** `SemanticVerifierPort` 定义在 `onto-assurance-runtime`，实现放在 `onto-ironclaw-adapter`。不在 IronClaw 内部定义接口，否则控制权倒置

10. **★★★ IronClaw 双重角色:** Lane A (Agent Execution) 被 OntoLoop 调用；Lane B (Semantic Verification) 被 OntoAssure Verification Fabric 调用。同一个 IronClaw，两种调用模式和约束

11. **★★★ 能力归属:** OpenCodeReview 被吸收的不是"代码审查能力"，而是"如何把非确定性的 Verifier 装进确定性的验证流水线"这一工程方法。这些方法归属于 OntoAssure，不归属于 IronClaw

6. **规则包是版本化实体:** RulePack.pack_hash 确保"使用不同版本的规则审查"意味着不同结果

7. **协议保持引用语义:** Go OntoFlow 只发送引用（contract_ref, rule_pack_refs），不发送完整内容。Rust 侧从 artifact store 拉取

8. **位置解析是纯函数:** 不依赖 LLM、不依赖网络、不依赖运行时状态。相同输入 → 相同输出（利于 replay/reproduce）
