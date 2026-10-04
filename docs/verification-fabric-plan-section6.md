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
