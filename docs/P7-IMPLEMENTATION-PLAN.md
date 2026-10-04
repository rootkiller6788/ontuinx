# P7 实施计划：OntoAssure Graph Verifier 接入

> **依据**：`OntoFirmwareGraph-PLAN.md` P7 架构 + 代码现状  
> **日期**：2026-07-28  
> **状态**：待审核  
> **预估**：5–7 天

---

## 一、总架构

```
L1 OntoAssure · Rust
  │
  ├── Discovery → Scope → Rules → Plan → Session → Schedule
  ├── Compiler Verifier (已有)
  ├── Test Verifier (已有)
  ├── Static Analysis Verifier (已有)
  ├── Semantic Review Verifier (已有 stub)
  │
  ├── Graph Integrity Verifier  ← P7 新增──┐
  └── Graph Risk Verifier       ← P7 新增──┤
                                           │ 确定性 gRPC 调用
                                           ▼
                                 ┌──────────────────────┐
                                 │ OntoCodeGraph        │
                                 │ (ontofirmwaregraphd) │
                                 │                      │
                                 │ PostgreSQL持久化      │
                                 │ CBM-derived C Core   │
                                 │ 无前端/无MCP          │
                                 └──────────────────────┘
```

P7 **只做 L1 这两根箭头**。调用权完全属于 OntoAssure VerifierScheduler，Agent 不能触发。

---

## 二、Step 1：扩展 VerifierError

**文件**：`crates/onto-assurance-runtime/src/verification/ports.rs`

**现状**（第 8 行）：
```rust
#[derive(Debug,thiserror::Error)] pub enum VerifierError {
    #[error("unavailable: {0}")] Unavailable(String),
    #[error("budget exhausted: {0}")] BudgetExhausted(String),
    #[error("failed: {0}")] Failed(String),
    #[error("timeout after {0}s")] Timeout(u64),
}
```

**改为**：
```rust
#[derive(Debug, thiserror::Error)]
pub enum VerifierError {
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
    #[error("failed: {0}")]
    Failed(String),
    #[error("timeout after {0}s")]
    Timeout(u64),

    // ── P7: Graph Verifier 专用 ──
    /// ofg-service 不可达 / PG 断连 / C core 崩溃
    #[error("environment error: {0}")]
    EnvironmentError(String),

    /// 快照存在但某些检查未通过（missing passes / unprocessed files）
    #[error("verification incomplete: {0}")]
    VerificationIncomplete(String),

    /// checkpoint hash / execution_generation 不匹配 OR 快照不存在
    #[error("stale snapshot: {0}")]
    StaleSnapshot(String),

    /// 语言或 pass 不在覆盖模型中
    #[error("unsupported coverage: {0}")]
    UnsupportedCoverage(String),
}
```

---

## 三、Step 2：ofg-service 新增 gRPC

### 3.1 新建文件：`crates/onto-graph-service/proto/snapshot_query.proto`

```protobuf
syntax = "proto3";
package ocg.snapshot_query.v1;

// ── 服务定义 ──
service SnapshotQueryService {
  rpc CheckIntegrity(CheckIntegrityRequest) returns (CheckIntegrityResponse);
}

// ── 请求 ──
message CheckIntegrityRequest {
  // 定位仓库和快照
  string repository_name = 1;            // required
  string base_commit_sha = 2;            // required
  string candidate_checkpoint_hash = 3;  // required
  uint64 execution_generation = 4;       // required

  // 要检查的变更文件（相对路径）
  repeated string changed_files = 5;

  // 要求覆盖的 pass 名称列表（如 ["parse", "extract", "resolve"]）
  repeated string required_passes = 6;
}

// ── 响应 ──
message CheckIntegrityResponse {
  // 逐项检查结果
  bool snapshot_exists = 1;
  bool is_sealed = 2;
  bool checkpoint_matches = 3;
  bool generation_matches = 4;
  bool all_files_processed = 5;
  bool coverage_complete = 6;

  // 快照元数据（存在时填充）
  string snapshot_id = 7;
  string graph_content_hash = 8;
  int64 node_count = 9;
  int64 edge_count = 10;
  string coverage_status = 11;

  // 未满足项详情
  repeated string unprocessed_files = 12;
  repeated string missing_passes = 13;
  string error_detail = 14;              // 服务侧异常时才非空
}
```

### 3.2 新建文件：`crates/onto-graph-service/proto/graph_risk.proto`

```protobuf
syntax = "proto3";
package ocg.graph_risk.v1;

// ── 服务定义 ──
service GraphRiskService {
  rpc AnalyzeCallChain(AnalyzeCallChainRequest) returns (AnalyzeCallChainResponse);
  rpc GetDependencyClosure(GetDependencyClosureRequest) returns (GetDependencyClosureResponse);
  rpc GetAffectedTests(GetAffectedTestsRequest) returns (GetAffectedTestsResponse);
  rpc GetSharedStateAccess(GetSharedStateAccessRequest) returns (GetSharedStateAccessResponse);
  rpc GetLanguageRisks(GetLanguageRisksRequest) returns (GetLanguageRisksResponse);
}

// ── 公共类型 ──
message EntityRef {
  string entity_key = 1;           // stable_key
  string qualified_name = 2;
  string entity_kind = 3;          // "Function" | "Method" | "Variable" | ...
  string file_path = 4;
  int32 start_line = 5;
}

// ── 调用链分析 ──
message AnalyzeCallChainRequest {
  string snapshot_id = 1;
  repeated string changed_entity_keys = 2;
  int32 max_depth = 3;             // 默认 3，上限 5
  int32 max_results = 4;           // 默认 50
}

message CallChainEntry {
  EntityRef entity = 1;
  string edge_kind = 2;            // "CALLS" | "INSTANTIATES" | "OVERRIDES" | "IMPLEMENTS"
  string direction = 3;            // "caller" | "callee"
  int32 depth = 4;
}

message AnalyzeCallChainResponse {
  repeated CallChainEntry entries = 1;
  int32 total_affected = 2;
}

// ── 依赖闭包 ──
message GetDependencyClosureRequest {
  string snapshot_id = 1;
  repeated string changed_entity_keys = 2;
  bool transitive = 3;             // 是否递归
  int32 max_results = 4;
}

message GetDependencyClosureResponse {
  repeated EntityRef direct_dependencies = 1;
  repeated EntityRef dependents = 2;   // 谁依赖 changed entities
  int32 closure_size = 3;
}

// ── 受影响测试 ──
message GetAffectedTestsRequest {
  string snapshot_id = 1;
  repeated string changed_entity_keys = 2;
}

message AffectedTest {
  EntityRef test_entity = 1;       // 测试实体
  string relationship = 2;         // "direct_caller" | "indirect_dependency"
  int32 distance = 3;              // 距离 changed entity 的跳数
}

message GetAffectedTestsResponse {
  repeated AffectedTest tests = 1;
}

// ── 共享状态访问 ──
message GetSharedStateAccessRequest {
  string snapshot_id = 1;
  repeated string changed_entity_keys = 2;
}

message SharedStateAccess {
  EntityRef variable = 1;          // 共享变量
  string access_kind = 2;          // "READ" | "WRITE" | "READ_WRITE"
  EntityRef accessed_by = 3;       // 哪个实体访问了它
}

message GetSharedStateAccessResponse {
  repeated SharedStateAccess accesses = 1;
}

// ── 语言专项风险 ──
message GetLanguageRisksRequest {
  string snapshot_id = 1;
  repeated string changed_entity_keys = 2;
  string language = 3;             // "rust" | "c" | "cpp" | "python" | ...
}

message LanguageRisk {
  string risk_type = 1;            // "unsafe_block" | "reflection" | "dynamic_import"
                                   // | "fn_ptr_table" | "monkey_patch" | "macro_invocation"
  EntityRef entity = 2;
  string detail = 3;
  string severity = 4;             // "low" | "medium" | "high" | "critical"
}

message GetLanguageRisksResponse {
  repeated LanguageRisk risks = 1;
}
```

### 3.3 新建文件：`crates/onto-graph-service/src/snapshot_query_service.rs`

```rust
//! gRPC SnapshotQueryService 实现 — P7

use crate::pg_store::PgStore;
use ocg::snapshot_query::v1::{
    snapshot_query_service_server::SnapshotQueryService as SqsTrait,
    CheckIntegrityRequest, CheckIntegrityResponse,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

pub struct SnapshotQueryServiceImpl {
    pool: PgPool,
}

impl SnapshotQueryServiceImpl {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[tonic::async_trait]
impl SqsTrait for SnapshotQueryServiceImpl {
    async fn check_integrity(
        &self,
        req: Request<CheckIntegrityRequest>,
    ) -> Result<Response<CheckIntegrityResponse>, Status> {
        let r = req.into_inner();
        info!(
            repo = %r.repository_name, sha = %r.base_commit_sha,
            checkpoint = %r.candidate_checkpoint_hash, gen = r.execution_generation,
            "CheckIntegrity"
        );

        // 1. 查找快照
        let snap = sqlx::query_as::<_, SnapshotRow>(
            "SELECT snapshot_id, state, execution_generation,
                    candidate_checkpoint_hash, graph_content_hash,
                    node_count, edge_count, coverage_status
             FROM ofg_snapshot s
             JOIN ofg_repository r ON s.repository_id = r.repository_id
             WHERE r.name = $1
               AND s.base_commit_sha = $2
               AND s.candidate_checkpoint_hash = $3
             ORDER BY s.created_at DESC
             LIMIT 1"
        )
        .bind(&r.repository_name)
        .bind(&r.base_commit_sha)
        .bind(&r.candidate_checkpoint_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            warn!("PG error: {}", e);
            Status::internal("database error")
        })?;

        let mut resp = CheckIntegrityResponse::default();

        let snap = match snap {
            Some(s) => {
                resp.snapshot_exists = true;
                s
            }
            None => {
                resp.error_detail = format!(
                    "no snapshot for repo={} sha={} checkpoint={}",
                    r.repository_name, r.base_commit_sha, r.candidate_checkpoint_hash
                );
                return Ok(Response::new(resp));
            }
        };

        // 2. 检查状态
        resp.is_sealed = snap.state == "SEALED";
        resp.checkpoint_matches = snap.candidate_checkpoint_hash == r.candidate_checkpoint_hash;
        resp.generation_matches = snap.execution_generation == r.execution_generation as i64;
        resp.snapshot_id = snap.snapshot_id;
        resp.graph_content_hash = snap.graph_content_hash;
        resp.node_count = snap.node_count;
        resp.edge_count = snap.edge_count;
        resp.coverage_status = snap.coverage_status;

        // 3. 检查 changed files 是否全部处理
        let snapshot_id = snap.snapshot_id.clone();
        if !r.changed_files.is_empty() {
            let processed: Vec<String> = sqlx::query_as::<_, (String,)>(
                "SELECT f.rel_path
                 FROM ofg_file f
                 JOIN ofg_file_version fv ON f.file_id = fv.file_id
                 WHERE fv.snapshot_id = $1::uuid
                   AND f.rel_path = ANY($2)"
            )
            .bind(&snapshot_id)
            .bind(&r.changed_files)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("file query: {}", e)))?
            .into_iter()
            .map(|(p,)| p)
            .collect();

            resp.all_files_processed = processed.len() == r.changed_files.len();
            resp.unprocessed_files = r.changed_files.iter()
                .filter(|f| !processed.contains(f))
                .cloned()
                .collect();
        } else {
            resp.all_files_processed = true; // 无变更文件视为已处理
        }

        // 4. 检查覆盖率
        if !r.required_passes.is_empty() {
            let covered: Vec<String> = sqlx::query_as::<_, (String,)>(
                "SELECT DISTINCT pass_name
                 FROM ofg_pass_coverage
                 WHERE snapshot_id = $1::uuid
                   AND pass_name = ANY($2)
                   AND status = 'COMPLETE'"
            )
            .bind(&snapshot_id)
            .bind(&r.required_passes)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("coverage query: {}", e)))?
            .into_iter()
            .map(|(p,)| p)
            .collect();

            resp.coverage_complete = covered.len() == r.required_passes.len();
            resp.missing_passes = r.required_passes.iter()
                .filter(|p| !covered.contains(p))
                .cloned()
                .collect();
        } else {
            resp.coverage_complete = true; // 无要求 pass 视为完整
        }

        Ok(Response::new(resp))
    }
}

// ── 内部查询行 ──
struct SnapshotRow {
    snapshot_id: String,
    state: String,
    execution_generation: i64,
    candidate_checkpoint_hash: String,
    graph_content_hash: String,
    node_count: i64,
    edge_count: i64,
    coverage_status: String,
}
```

### 3.4 新建文件：`crates/onto-graph-service/src/graph_risk_service.rs`

```rust
//! gRPC GraphRiskService 实现 — P7

use ocg::graph_risk::v1::{
    graph_risk_service_server::GraphRiskService as GrsTrait,
    AnalyzeCallChainRequest, AnalyzeCallChainResponse, CallChainEntry, EntityRef,
    GetDependencyClosureRequest, GetDependencyClosureResponse,
    GetAffectedTestsRequest, GetAffectedTestsResponse, AffectedTest,
    GetSharedStateAccessRequest, GetSharedStateAccessResponse, SharedStateAccess,
    GetLanguageRisksRequest, GetLanguageRisksResponse, LanguageRisk,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct GraphRiskServiceImpl {
    pool: PgPool,
}

impl GraphRiskServiceImpl {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[tonic::async_trait]
impl GrsTrait for GraphRiskServiceImpl {
    // ── 调用链分析 ──
    async fn analyze_call_chain(
        &self, req: Request<AnalyzeCallChainRequest>,
    ) -> Result<Response<AnalyzeCallChainResponse>, Status> {
        let r = req.into_inner();
        let depth = r.max_depth.min(5).max(1);
        info!(snapshot_id = %r.snapshot_id, entities = ?r.changed_entity_keys, depth, "AnalyzeCallChain");

        let mut entries = Vec::new();

        // 向上查调用者（递归 CTE）
        let callers = sqlx::query_as::<_, CallChainRow>(
            "WITH RECURSIVE caller_chain AS (
                SELECT e.source_entity_id, e.edge_kind, 1 as depth
                FROM ofg_edge e
                JOIN ofg_entity tgt ON e.target_entity_id = tgt.entity_id
                WHERE e.snapshot_id = $1::uuid
                  AND tgt.stable_key = ANY($2)
                UNION ALL
                SELECT e.source_entity_id, e.edge_kind, cc.depth + 1
                FROM ofg_edge e
                JOIN caller_chain cc ON e.target_entity_id = cc.source_entity_id
                WHERE e.snapshot_id = $1::uuid AND cc.depth < $3
            )
            SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                   f.rel_path, ev.start_line, cc.edge_kind, cc.depth, 'caller' as direction
            FROM caller_chain cc
            JOIN ofg_entity ent ON cc.source_entity_id = ent.entity_id
            JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
            LEFT JOIN ofg_file f ON ev.file_id = f.file_id
            LIMIT $4"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(depth).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("call chain: {}", e)))?;

        entries.extend(into_call_chain_entries(callers));

        // 向下查被调用者
        let callees = sqlx::query_as::<_, CallChainRow>(
            "WITH RECURSIVE callee_chain AS (
                SELECT e.target_entity_id as source_entity_id, e.edge_kind, 1 as depth
                FROM ofg_edge e
                JOIN ofg_entity src ON e.source_entity_id = src.entity_id
                WHERE e.snapshot_id = $1::uuid
                  AND src.stable_key = ANY($2)
                UNION ALL
                SELECT e.target_entity_id, e.edge_kind, cc.depth + 1
                FROM ofg_edge e
                JOIN callee_chain cc ON e.source_entity_id = cc.source_entity_id
                WHERE e.snapshot_id = $1::uuid AND cc.depth < $3
            )
            SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                   f.rel_path, ev.start_line, cc.edge_kind, cc.depth, 'callee' as direction
            FROM callee_chain cc
            JOIN ofg_entity ent ON cc.source_entity_id = ent.entity_id
            JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
            LEFT JOIN ofg_file f ON ev.file_id = f.file_id
            LIMIT $4"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(depth).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("call chain: {}", e)))?;

        entries.extend(into_call_chain_entries(callees));

        Ok(Response::new(AnalyzeCallChainResponse {
            total_affected: entries.len() as i32,
            entries,
        }))
    }

    // ── 依赖闭包 ──
    async fn get_dependency_closure(
        &self, req: Request<GetDependencyClosureRequest>,
    ) -> Result<Response<GetDependencyClosureResponse>, Status> {
        let r = req.into_inner();
        info!(snapshot_id = %r.snapshot_id, entities = ?r.changed_entity_keys, "GetDependencyClosure");

        // 查询 direct dependencies（imports / depends_on 类型边）
        let deps = sqlx::query_as::<_, EntityRefRow>(
            "SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line
             FROM ofg_edge e
             JOIN ofg_entity src ON e.source_entity_id = src.entity_id
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src.stable_key = ANY($2)
               AND e.edge_kind IN ('IMPORTS', 'DEPENDS_ON', 'INCLUDES', 'USES_TYPE')
             LIMIT $3"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("dep closure: {}", e)))?;

        let direct: Vec<EntityRef> = deps.into_iter().map(|r| EntityRef {
            entity_key: r.stable_key, qualified_name: r.qualified_name,
            entity_kind: r.entity_kind, file_path: r.rel_path.unwrap_or_default(),
            start_line: r.start_line,
        }).collect();

        Ok(Response::new(GetDependencyClosureResponse {
            closure_size: direct.len() as i32,
            direct_dependencies: direct,
            dependents: vec![], // P7 MVP: 暂不反查（性能）
        }))
    }

    // ── 受影响测试 ──
    async fn get_affected_tests(
        &self, req: Request<GetAffectedTestsRequest>,
    ) -> Result<Response<GetAffectedTestsResponse>, Status> {
        let r = req.into_inner();
        info!(snapshot_id = %r.snapshot_id, entities = ?r.changed_entity_keys, "GetAffectedTests");

        let tests = sqlx::query_as::<_, TestRow>(
            "SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line,
                    CASE WHEN e.edge_kind = 'CALLS' THEN 'direct_caller'
                         ELSE 'indirect_dependency' END as relationship,
                    CASE WHEN e.edge_kind = 'CALLS' THEN 1 ELSE 2 END as distance
             FROM ofg_edge e
             JOIN ofg_entity src ON e.source_entity_id = src.entity_id
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src.stable_key = ANY($2)
               AND ent.entity_kind IN ('Test', 'TestCase', 'TestSuite', 'TestFunction')
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("affected tests: {}", e)))?;

        Ok(Response::new(GetAffectedTestsResponse {
            tests: tests.into_iter().map(|t| AffectedTest {
                test_entity: Some(EntityRef {
                    entity_key: t.stable_key,
                    qualified_name: t.qualified_name,
                    entity_kind: t.entity_kind,
                    file_path: t.rel_path.unwrap_or_default(),
                    start_line: t.start_line,
                }),
                relationship: t.relationship,
                distance: t.distance,
            }).collect(),
        }))
    }

    // ── 共享状态访问 ──
    async fn get_shared_state_access(
        &self, req: Request<GetSharedStateAccessRequest>,
    ) -> Result<Response<GetSharedStateAccessResponse>, Status> {
        let r = req.into_inner();
        info!(snapshot_id = %r.snapshot_id, entities = ?r.changed_entity_keys, "GetSharedStateAccess");

        let accesses = sqlx::query_as::<_, SharedStateRow>(
            "SELECT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line,
                    e.edge_kind as access_kind,
                    src_ent.stable_key as accessed_by_key
             FROM ofg_edge e
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity src_ent ON e.source_entity_id = src_ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src_ent.stable_key = ANY($2)
               AND e.edge_kind IN ('READS', 'WRITES')
               AND ent.entity_kind IN ('Variable', 'GlobalVariable', 'StaticVariable', 'Singleton')
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("shared state: {}", e)))?;

        Ok(Response::new(GetSharedStateAccessResponse {
            accesses: accesses.into_iter().map(|a| SharedStateAccess {
                variable: Some(EntityRef {
                    entity_key: a.stable_key,
                    qualified_name: a.qualified_name,
                    entity_kind: a.entity_kind,
                    file_path: a.rel_path.unwrap_or_default(),
                    start_line: a.start_line,
                }),
                access_kind: a.access_kind,
                accessed_by: Some(EntityRef {
                    entity_key: a.accessed_by_key,
                    ..Default::default()
                }),
            }).collect(),
        }))
    }

    // ── 语言专项风险 ──
    async fn get_language_risks(
        &self, req: Request<GetLanguageRisksRequest>,
    ) -> Result<Response<GetLanguageRisksResponse>, Status> {
        let r = req.into_inner();
        info!(snapshot_id = %r.snapshot_id, language = %r.language, "GetLanguageRisks");

        let risks = sqlx::query_as::<_, LanguageRiskRow>(
            "SELECT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line,
                    ev.properties->>'unsafe' as unsafe_flag,
                    ev.properties->>'reflection' as reflection_flag,
                    ev.properties->>'dynamic_import' as dynamic_import_flag,
                    ev.properties->>'fn_ptr_table' as fn_ptr_table_flag
             FROM ofg_entity ent
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE ent.stable_key = ANY($2)
               AND ent.language = $3
               AND (
                   ev.properties->>'unsafe' = 'true'
                   OR ev.properties->>'reflection' = 'true'
                   OR ev.properties->>'dynamic_import' = 'true'
                   OR ev.properties->>'fn_ptr_table' = 'true'
               )
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(&r.language)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("language risks: {}", e)))?;

        let mut results = Vec::new();
        for row in risks {
            if row.unsafe_flag == Some("true".into()) {
                results.push(make_risk(&row, "unsafe_block", "critical"));
            }
            if row.reflection_flag == Some("true".into()) {
                results.push(make_risk(&row, "reflection", "high"));
            }
            if row.dynamic_import_flag == Some("true".into()) {
                results.push(make_risk(&row, "dynamic_import", "medium"));
            }
            if row.fn_ptr_table_flag == Some("true".into()) {
                results.push(make_risk(&row, "fn_ptr_table", "high"));
            }
        }

        Ok(Response::new(GetLanguageRisksResponse { risks: results }))
    }
}

// ── 内部查询行 ──
struct CallChainRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    edge_kind: String, depth: i32, direction: String,
}
fn into_call_chain_entries(rows: Vec<CallChainRow>) -> Vec<CallChainEntry> {
    rows.into_iter().map(|r| CallChainEntry {
        entity: Some(EntityRef {
            entity_key: r.stable_key, qualified_name: r.qualified_name,
            entity_kind: r.entity_kind, file_path: r.rel_path.unwrap_or_default(),
            start_line: r.start_line,
        }),
        edge_kind: r.edge_kind, direction: r.direction, depth: r.depth,
    }).collect()
}

struct EntityRefRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
}

struct TestRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    relationship: String, distance: i32,
}

struct SharedStateRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    access_kind: String, accessed_by_key: String,
}

struct LanguageRiskRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    unsafe_flag: Option<String>, reflection_flag: Option<String>,
    dynamic_import_flag: Option<String>, fn_ptr_table_flag: Option<String>,
}

fn make_risk(row: &LanguageRiskRow, risk_type: &str, severity: &str) -> LanguageRisk {
    LanguageRisk {
        risk_type: risk_type.into(), severity: severity.into(),
        detail: format!("{} {} in {}", row.entity_kind, row.qualified_name,
                        row.rel_path.as_deref().unwrap_or("?")),
        entity: Some(EntityRef {
            entity_key: row.stable_key.clone(),
            qualified_name: row.qualified_name.clone(),
            entity_kind: row.entity_kind.clone(),
            file_path: row.rel_path.clone().unwrap_or_default(),
            start_line: row.start_line,
        }),
    }
}
```

### 3.5 修改 `crates/onto-graph-service/Cargo.toml`

在 `[build-dependencies]` 后不变（`tonic-build` 已存在），在 `[dependencies]` 中确认已有 `sqlx`, `uuid`, `tonic`, `prost`。

### 3.6 修改 `crates/onto-graph-service/build.rs`

在现有的 `tonic_build::compile_protos("proto/health.proto")` 基础上追加编译三个 proto：

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .compile_protos(
            &["proto/health.proto",
              "proto/snapshot_query.proto",
              "proto/graph_risk.proto"],
            &["proto"],
        )?;
    Ok(())
}
```

### 3.7 修改 `crates/onto-graph-service/src/main.rs`

注册两个新 service：

```rust
mod snapshot_query_service;
mod graph_risk_service;
// ... existing mod declarations ...

use snapshot_query_service::SnapshotQueryServiceImpl;
use graph_risk_service::GraphRiskServiceImpl;

// ... in main():
let pool = /* existing pool setup */;
let snapshot_svc = SnapshotQueryServiceImpl::new(pool.clone());
let risk_svc = GraphRiskServiceImpl::new(pool.clone());

Server::builder()
    .add_service(HealthServer::new(health))
    .add_service(SnapshotQueryServiceServer::new(snapshot_svc))
    .add_service(GraphRiskServiceServer::new(risk_svc))
    .serve(addr).await?;
```

### 3.8 修改 `crates/onto-graph-service/proto/` 的 package 命名

将 `health.proto` 的 package 改为 `ocg.health.v1` 以统一命名空间（可选，降低优先级）。

---

## 四、Step 3：onto-graph-verifiers crate

### 4.1 `Cargo.toml`

```toml
[package]
name = "onto-graph-verifiers"
version = "0.1.0"
edition = "2021"
description = "P7: Graph Integrity & Graph Risk verifiers for OntoAssure"

[dependencies]
onto-assurance-types = { path = "../onto-assurance-types" }
onto-assurance-runtime = { path = "../onto-assurance-runtime" }
tonic = "0.12"
prost = "0.13"
tokio = { version = "1", features = ["macros", "rt"] }
async-trait = "0.1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
```

### 4.2 `src/lib.rs`

```rust
pub mod client;
pub mod graph_integrity;
pub mod graph_risk;
pub mod schedule_rules;
pub mod change_classifier;

pub use graph_integrity::GraphIntegrityVerifier;
pub use graph_risk::{GraphRiskVerifier, RiskDimensions, RiskDimension};
pub use schedule_rules::{ChangeType, EnforcementLevel, classify_change, integrity_policy, risk_policy};
pub use client::{SnapshotQueryClient, GraphRiskClient, GraphClientError};
```

### 4.3 `src/client.rs` — 完整接口定义

```rust
//! gRPC 客户端抽象 — 摹仿 onto-code-pack/src/build_verifier.rs 的 CommandRunner 模式
//!
//! 定义 trait → tonic 生产实现 + mock 测试实现

use async_trait::async_trait;
use std::sync::Arc;

// ── 错误类型 ──
#[derive(Debug, thiserror::Error)]
pub enum GraphClientError {
    #[error("gRPC connection failed: {0}")]
    ConnectionFailed(String),
    #[error("gRPC call failed: {0}")]
    GrpcError(String),
    #[error("snapshot not found: {0}")]
    NotFound(String),
}

// ── 从 Proto 重导出响应类型（避免复制定义）──
// 实际编译时由 tonic-build 生成在 ocg::snapshot_query::v1 / ocg::graph_risk::v1
// 此处为文档用 alias。生产代码直接 use tonic 生成的模块。
pub type IntegrityResult = ocg::snapshot_query::v1::CheckIntegrityResponse;
pub type CallChainResult = ocg::graph_risk::v1::AnalyzeCallChainResponse;
pub type DependencyResult = ocg::graph_risk::v1::GetDependencyClosureResponse;
pub type AffectedTestsResult = ocg::graph_risk::v1::GetAffectedTestsResponse;
pub type SharedStateResult = ocg::graph_risk::v1::GetSharedStateAccessResponse;
pub type LanguageRiskResult = ocg::graph_risk::v1::GetLanguageRisksResponse;

// ── 完整性查询客户端 ──
#[async_trait]
pub trait SnapshotQueryClient: Send + Sync {
    async fn check_integrity(
        &self,
        repository_name: &str,
        base_commit_sha: &str,
        candidate_checkpoint_hash: &str,
        execution_generation: u64,
        changed_files: &[String],
        required_passes: &[String],
    ) -> Result<IntegrityResult, GraphClientError>;
}

// ── 风险分析客户端 ──
#[async_trait]
pub trait GraphRiskClient: Send + Sync {
    async fn analyze_call_chain(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        max_depth: i32, max_results: i32,
    ) -> Result<CallChainResult, GraphClientError>;

    async fn get_dependency_closure(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        transitive: bool, max_results: i32,
    ) -> Result<DependencyResult, GraphClientError>;

    async fn get_affected_tests(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
    ) -> Result<AffectedTestsResult, GraphClientError>;

    async fn get_shared_state_access(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
    ) -> Result<SharedStateResult, GraphClientError>;

    async fn get_language_risks(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        language: &str,
    ) -> Result<LanguageRiskResult, GraphClientError>;
}

// ── 生产实现（tonic）──
// 由 tonic-build 生成，此处省略具体连接代码。
// 模式：
//   pub struct TonicSnapshotClient { inner: SnapshotQueryServiceClient<Channel> }
//   impl SnapshotQueryClient for TonicSnapshotClient { ... }
//   pub struct TonicRiskClient { inner: GraphRiskServiceClient<Channel> }
//   impl GraphRiskClient for TonicRiskClient { ... }

// ── Mock 实现（测试用）──
pub struct MockSnapshotClient {
    pub response: IntegrityResult,
    pub error: Option<GraphClientError>,
}

#[async_trait]
impl SnapshotQueryClient for MockSnapshotClient {
    async fn check_integrity(&self, _: &str, _: &str, _: &str, _: u64,
        _: &[String], _: &[String],
    ) -> Result<IntegrityResult, GraphClientError> {
        if let Some(ref e) = self.error {
            return Err(match e {
                GraphClientError::ConnectionFailed(s) => GraphClientError::ConnectionFailed(s.clone()),
                GraphClientError::GrpcError(s) => GraphClientError::GrpcError(s.clone()),
                GraphClientError::NotFound(s) => GraphClientError::NotFound(s.clone()),
            });
        }
        Ok(self.response.clone())
    }
}

pub struct MockRiskClient {
    pub call_chain: CallChainResult,
    pub dependency: DependencyResult,
    pub affected_tests: AffectedTestsResult,
    pub shared_state: SharedStateResult,
    pub language_risks: LanguageRiskResult,
    pub error: Option<GraphClientError>,
}

// MockRiskClient 同样 impl GraphRiskClient，每个方法返回对应 field 或 error
```

### 4.4 `src/schedule_rules.rs` — 纯函数映射

```rust
//! 调度规则 — 纯函数，无 I/O，可直接单元测试

/// 变更类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeType {
    Documentation,      // *.md, docs/, README
    Config,             // *.toml, *.yaml, *.json, *.cfg
    TestCode,           // *test*, *spec*, tests/
    NormalCode,         // 普通源码
    PublicApi,          // pub fn, pub struct, __all__, exports
    SecurityCritical,   // auth, payment, crypto, permission
    Unknown,            // 无法分类 → 保守
}

/// 强制执行等级
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementLevel {
    Skip,                  // 不调用 Verifier
    Advisory,              // 调用但失败不阻断
    Required,              // 必须通过
    StrengthenedRequired,  // 必须通过 + 额外检查
}

impl EnforcementLevel {
    pub fn should_invoke(self) -> bool { !matches!(self, Self::Skip) }
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Required | Self::StrengthenedRequired)
    }
}

/// 分类变更类型（启发式）
pub fn classify_change(changed_files: &[String]) -> ChangeType {
    if changed_files.is_empty() { return ChangeType::Unknown; }

    let mut highest = ChangeType::Documentation;
    for f in changed_files {
        let ct = classify_single(f);
        if ct > highest { highest = ct; }
    }
    highest
}

fn classify_single(path: &str) -> ChangeType {
    let lower = path.to_lowercase();

    // 安全关键路径
    if lower.contains("auth") || lower.contains("payment") || lower.contains("crypto")
        || lower.contains("permission") || lower.contains("credential")
        || lower.contains("token") || lower.contains("secret") {
        return ChangeType::SecurityCritical;
    }

    // 文档
    if lower.ends_with(".md") || lower.ends_with(".rst") || lower.ends_with(".txt")
        || lower.starts_with("docs/") || lower.contains("readme") {
        return ChangeType::Documentation;
    }

    // 配置
    if lower.ends_with(".toml") || lower.ends_with(".yaml") || lower.ends_with(".yml")
        || lower.ends_with(".json") || lower.ends_with(".cfg") || lower.ends_with(".ini")
        || lower.ends_with(".env.example") {
        return ChangeType::Config;
    }

    // 测试
    if lower.contains("test") || lower.contains("spec") || lower.contains("__test__")
        || lower.starts_with("tests/") || lower.ends_with("_test.rs")
        || lower.ends_with("_test.py") || lower.ends_with("_test.go")
        || lower.ends_with(".test.ts") || lower.ends_with(".spec.ts") {
        return ChangeType::TestCode;
    }

    // 公共 API 标记（通过文件名推断）
    if lower.contains("__init__") || lower.contains("index.")
        || lower.contains("lib.rs") || lower.contains("mod.rs")
        || lower.contains("public") || lower.contains("api/") {
        return ChangeType::PublicApi;
    }

    // 其余 → 普通代码
    ChangeType::NormalCode
}

/// Graph Integrity 策略
pub fn integrity_policy(ct: ChangeType) -> EnforcementLevel {
    match ct {
        ChangeType::Documentation    => EnforcementLevel::Skip,
        ChangeType::Config           => EnforcementLevel::Advisory,
        ChangeType::TestCode         => EnforcementLevel::Required,
        ChangeType::NormalCode       => EnforcementLevel::Required,
        ChangeType::PublicApi        => EnforcementLevel::Required,
        ChangeType::SecurityCritical => EnforcementLevel::StrengthenedRequired,
        ChangeType::Unknown          => EnforcementLevel::Required, // 保守：未知 = 必须
    }
}

/// Graph Risk 策略
pub fn risk_policy(ct: ChangeType) -> EnforcementLevel {
    match ct {
        ChangeType::Documentation    => EnforcementLevel::Skip,
        ChangeType::Config           => EnforcementLevel::Advisory,
        ChangeType::TestCode         => EnforcementLevel::Advisory,
        ChangeType::NormalCode       => EnforcementLevel::Required,
        ChangeType::PublicApi        => EnforcementLevel::StrengthenedRequired,
        ChangeType::SecurityCritical => EnforcementLevel::StrengthenedRequired,
        ChangeType::Unknown          => EnforcementLevel::Required, // 保守
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readme_is_docs() {
        assert_eq!(classify_change(&["README.md".into()]), ChangeType::Documentation);
    }

    #[test]
    fn test_auth_is_security() {
        assert_eq!(classify_change(&["src/auth/login.rs".into()]), ChangeType::SecurityCritical);
    }

    #[test]
    fn test_mixed_chooses_highest() {
        assert_eq!(
            classify_change(&["README.md".into(), "src/auth/payment.rs".into()]),
            ChangeType::SecurityCritical
        );
    }

    #[test]
    fn test_docs_policy_is_skip() {
        assert_eq!(integrity_policy(ChangeType::Documentation), EnforcementLevel::Skip);
        assert_eq!(risk_policy(ChangeType::Documentation), EnforcementLevel::Skip);
    }

    #[test]
    fn test_security_policy_is_strengthened() {
        assert_eq!(integrity_policy(ChangeType::SecurityCritical), EnforcementLevel::StrengthenedRequired);
        assert_eq!(risk_policy(ChangeType::SecurityCritical), EnforcementLevel::StrengthenedRequired);
    }

    #[test]
    fn test_unknown_is_required() {
        assert_eq!(integrity_policy(ChangeType::Unknown), EnforcementLevel::Required);
    }
}
```

### 4.5 `src/graph_integrity.rs`

```rust
//! GraphIntegrityVerifier — P7
//!
//! 所有代码变更强制执行。检查：
//!   1. Candidate 有图谱快照
//!   2. 快照 SEALED
//!   3. checkpoint hash 一致
//!   4. execution_generation 一致
//!   5. changed files 已处理
//!   6. required passes 覆盖完整
//!
//! 图谱不可用 → EnvironmentError / VerificationIncomplete / StaleSnapshot / UnsupportedCoverage
//! 绝不自动降级为 PASS（不返回 Ok(vec![]) 冒充通过）

use async_trait::async_trait;
use onto_assurance_runtime::verification::ports::{DeterministicVerifierPort, VerifierError};
use onto_assurance_types::finding::{FindingCandidate, FindingSeverity, FindingCategory};
use onto_assurance_types::verification_plan::VerificationUnit;
use std::sync::Arc;
use tracing::{info, warn};

use crate::client::{SnapshotQueryClient, GraphClientError};

pub struct GraphIntegrityVerifier {
    client: Arc<dyn SnapshotQueryClient>,
    /// 默认 required passes
    default_passes: Vec<String>,
}

impl GraphIntegrityVerifier {
    pub fn new(client: Arc<dyn SnapshotQueryClient>) -> Self {
        Self {
            client,
            default_passes: vec![
                "parse".into(), "extract".into(), "resolve".into(),
            ],
        }
    }
}

#[async_trait]
impl DeterministicVerifierPort for GraphIntegrityVerifier {
    async fn verify(
        &self,
        unit: &VerificationUnit,
    ) -> Result<Vec<FindingCandidate>, VerifierError> {
        // ── 1. 从 VerificationUnit.metadata 提取参数 ──
        let metadata = &unit.metadata;
        let repo = metadata.get("repository_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let base_sha = metadata.get("base_commit_sha")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let checkpoint = metadata.get("candidate_checkpoint_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let generation = metadata.get("execution_generation")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let changed_files: Vec<String> = metadata.get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let required_passes: Vec<String> = metadata.get("required_passes")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| self.default_passes.clone());

        // ── 2. gRPC 调用 ──
        let resp = self.client.check_integrity(
            repo, base_sha, checkpoint, generation,
            &changed_files, &required_passes,
        ).await;

        match resp {
            Err(GraphClientError::ConnectionFailed(e)) => {
                warn!("ofg-service unreachable: {}", e);
                return Err(VerifierError::EnvironmentError(format!(
                    "Graph service unreachable: {}. Candidate cannot be verified.", e
                )));
            }
            Err(GraphClientError::GrpcError(e)) => {
                warn!("gRPC error: {}", e);
                return Err(VerifierError::EnvironmentError(format!(
                    "Graph query failed: {}. Verification cannot proceed.", e
                )));
            }
            Err(GraphClientError::NotFound(_)) => {
                return Err(VerifierError::StaleSnapshot(format!(
                    "No snapshot for repo={} checkpoint={}. Candidate may not have been indexed.",
                    repo, checkpoint
                )));
            }
            Ok(resp) => {
                // ── 3. 逐字段检查 ──
                let mut findings = Vec::new();

                // 3a. 快照存在性
                if !resp.snapshot_exists {
                    return Err(VerifierError::StaleSnapshot(
                        "No graph snapshot found for this candidate.".into()
                    ));
                }

                // 3b. 快照未 SEALED
                if !resp.is_sealed {
                    return Err(VerifierError::StaleSnapshot(format!(
                        "Snapshot {} is not SEALED (state is not SEALED).", resp.snapshot_id
                    )));
                }

                // 3c. checkpoint 不匹配
                if !resp.checkpoint_matches {
                    return Err(VerifierError::StaleSnapshot(format!(
                        "Snapshot {} checkpoint mismatch.", resp.snapshot_id
                    )));
                }

                // 3d. generation 不匹配
                if !resp.generation_matches {
                    return Err(VerifierError::StaleSnapshot(format!(
                        "Snapshot {} generation mismatch: expected {}.",
                        resp.snapshot_id, generation
                    )));
                }

                // 3e. 有未处理的文件
                if !resp.all_files_processed {
                    for f in &resp.unprocessed_files {
                        findings.push(FindingCandidate {
                            finding_id: format!("graph-integrity-unprocessed-{}", uuid::Uuid::new_v4()),
                            target_id: unit.target_id.clone(),
                            rule_id: "graph_integrity.changed_files_processed".into(),
                            verifier_id: "graph-integrity".into(),
                            severity: FindingSeverity::Critical,
                            category: FindingCategory::Other,
                            message: format!("File '{}' was changed but has no graph coverage in snapshot {}.", f, resp.snapshot_id),
                            ..Default::default()
                        });
                    }
                    info!(unprocessed = ?resp.unprocessed_files, "files not fully processed");
                }

                // 3f. 覆盖率不完整
                if !resp.coverage_complete {
                    for p in &resp.missing_passes {
                        findings.push(FindingCandidate {
                            finding_id: format!("graph-integrity-missing-pass-{}", uuid::Uuid::new_v4()),
                            target_id: unit.target_id.clone(),
                            rule_id: "graph_integrity.coverage_complete".into(),
                            verifier_id: "graph-integrity".into(),
                            severity: FindingSeverity::High,
                            category: FindingCategory::Other,
                            message: format!(
                                "Required pass '{}' is not complete in snapshot {}. Coverage status: {}",
                                p, resp.snapshot_id, resp.coverage_status
                            ),
                            ..Default::default()
                        });
                    }
                    info!(missing = ?resp.missing_passes, "coverage incomplete");
                }

                // ── 4. 有 findings 时返回 VerificationIncomplete ──
                if !findings.is_empty() {
                    info!(count = findings.len(), "integrity gaps found");
                    return Err(VerifierError::VerificationIncomplete(format!(
                        "{} integrity gap(s) found in snapshot {}.",
                        findings.len(), resp.snapshot_id
                    )));
                }

                // ── 5. 全部通过 ──
                info!(snapshot_id = %resp.snapshot_id, nodes = resp.node_count,
                      edges = resp.edge_count, "GraphIntegrity PASSED");
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockSnapshotClient;
    use serde_json::json;

    fn make_unit() -> VerificationUnit {
        VerificationUnit {
            unit_id: "test-unit-1".into(),
            target_id: "test-target-1".into(),
            rule_set_id: "graph-integrity".into(),
            verifier_kind: onto_assurance_types::verification_plan::VerifierKind::Deterministic,
            priority: 1,
            depends_on: vec![],
            metadata: json!({
                "repository_name": "test-repo",
                "base_commit_sha": "abc123",
                "candidate_checkpoint_hash": "checkpoint-xyz",
                "execution_generation": 1,
                "changed_files": ["src/main.rs"],
                "required_passes": ["parse", "extract", "resolve"]
            }),
        }
    }

    #[tokio::test]
    async fn test_sealed_snapshot_passes() {
        let mut resp = crate::client::IntegrityResult::default();
        resp.snapshot_exists = true;
        resp.is_sealed = true;
        resp.checkpoint_matches = true;
        resp.generation_matches = true;
        resp.all_files_processed = true;
        resp.coverage_complete = true;
        resp.snapshot_id = "snap-1".into();

        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: resp, error: None }));
        let result = v.verify(&make_unit()).await;
        assert!(result.is_ok(), "SEALED snapshot should pass: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_not_sealed_is_stale() {
        let mut resp = crate::client::IntegrityResult::default();
        resp.snapshot_exists = true;
        resp.is_sealed = false;

        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: resp, error: None }));
        let result = v.verify(&make_unit()).await;
        assert!(matches!(result, Err(VerifierError::StaleSnapshot(_))),
                "NOT SEALED should be StaleSnapshot, got {:?}", result.err());
    }

    #[tokio::test]
    async fn test_connection_failure_is_environment_error() {
        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient {
            response: Default::default(),
            error: Some(GraphClientError::ConnectionFailed("timeout".into())),
        }));
        let result = v.verify(&make_unit()).await;
        assert!(matches!(result, Err(VerifierError::EnvironmentError(_))),
                "Connection failure should be EnvironmentError");
    }
}
```

### 4.6 `src/graph_risk.rs`

```rust
//! GraphRiskVerifier — P7
//!
//! 按策略调度，分析：
//!   - 调用链影响
//!   - 依赖闭包
//!   - 受影响测试
//!   - 共享状态访问
//!   - 语言专项风险

use async_trait::async_trait;
use onto_assurance_runtime::verification::ports::{DeterministicVerifierPort, VerifierError};
use onto_assurance_types::finding::{FindingCandidate, FindingSeverity, FindingCategory};
use onto_assurance_types::verification_plan::VerificationUnit;
use std::sync::Arc;
use tracing::{info, warn};

use crate::client::{GraphRiskClient, GraphClientError};
use crate::schedule_rules::EnforcementLevel;

/// 风险分析维度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDimension {
    CallChain,
    DependencyClosure,
    AffectedTests,
    SharedState,
    LanguageRisks,
}

#[derive(Debug, Clone)]
pub struct RiskDimensions {
    pub call_chain: bool,
    pub dependency_closure: bool,
    pub affected_tests: bool,
    pub shared_state: bool,
    pub language_risks: bool,
}

impl RiskDimensions {
    pub fn minimal() -> Self {
        Self { call_chain: true, dependency_closure: false,
               affected_tests: false, shared_state: false, language_risks: false }
    }
    pub fn full() -> Self {
        Self { call_chain: true, dependency_closure: true,
               affected_tests: true, shared_state: true, language_risks: true }
    }
    pub fn for_enforcement(level: EnforcementLevel) -> Self {
        match level {
            EnforcementLevel::Skip => Self { call_chain: false, dependency_closure: false,
                affected_tests: false, shared_state: false, language_risks: false },
            EnforcementLevel::Advisory | EnforcementLevel::Required => Self::minimal(),
            EnforcementLevel::StrengthenedRequired => Self::full(),
        }
    }
}

pub struct GraphRiskVerifier {
    client: Arc<dyn GraphRiskClient>,
    enforcement: EnforcementLevel,
    dimensions: RiskDimensions,
}

impl GraphRiskVerifier {
    pub fn new(client: Arc<dyn GraphRiskClient>, enforcement: EnforcementLevel) -> Self {
        let dimensions = RiskDimensions::for_enforcement(enforcement);
        Self { client, enforcement, dimensions }
    }
}

#[async_trait]
impl DeterministicVerifierPort for GraphRiskVerifier {
    async fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, VerifierError> {
        let metadata = &unit.metadata;
        let snapshot_id = metadata.get("snapshot_id")
            .and_then(|v| v.as_str()).unwrap_or("");
        let entity_keys: Vec<String> = metadata.get("changed_entity_keys")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let language = metadata.get("language")
            .and_then(|v| v.as_str()).unwrap_or("");

        if snapshot_id.is_empty() || entity_keys.is_empty() {
            return Ok(vec![]); // 无数据 → 跳过（GraphIntegrity 会捕获）
        }

        let mut findings = Vec::new();

        // ── 调用链分析 ──
        if self.dimensions.call_chain {
            match self.client.analyze_call_chain(snapshot_id, &entity_keys, 3, 50).await {
                Ok(resp) => {
                    let high_fanout: Vec<_> = resp.entries.iter()
                        .filter(|e| e.depth <= 2 && e.direction == "caller")
                        .collect();
                    if high_fanout.len() > 10 {
                        findings.push(FindingCandidate {
                            finding_id: format!("graph-risk-call-chain-{}", uuid::Uuid::new_v4()),
                            target_id: unit.target_id.clone(),
                            rule_id: "graph_risk.call_chain_impact".into(),
                            verifier_id: "graph-risk".into(),
                            severity: FindingSeverity::High,
                            category: FindingCategory::Maintainability,
                            message: format!(
                                "{} callers within depth 2. Changes may have wide impact.",
                                high_fanout.len()
                            ),
                            ..Default::default()
                        });
                    }
                }
                Err(e) => {
                    warn!("call chain analysis failed: {}", e);
                    if self.enforcement.is_blocking() {
                        return Err(VerifierError::EnvironmentError(format!("Call chain analysis failed: {}", e)));
                    }
                }
            }
        }

        // ── 语言专项风险 ──
        if self.dimensions.language_risks && !language.is_empty() {
            match self.client.get_language_risks(snapshot_id, &entity_keys, language).await {
                Ok(resp) => {
                    for risk in &resp.risks {
                        findings.push(FindingCandidate {
                            finding_id: format!("graph-risk-lang-{}", uuid::Uuid::new_v4()),
                            target_id: unit.target_id.clone(),
                            rule_id: format!("graph_risk.language.{}", risk.risk_type),
                            verifier_id: "graph-risk".into(),
                            severity: match risk.severity.as_str() {
                                "critical" => FindingSeverity::Critical,
                                "high" => FindingSeverity::High,
                                _ => FindingSeverity::Medium,
                            },
                            category: FindingCategory::Security,
                            message: format!("[{}] {}", risk.risk_type, risk.detail),
                            ..Default::default()
                        });
                    }
                }
                Err(e) => {
                    warn!("language risk analysis failed: {}", e);
                    if self.enforcement.is_blocking() {
                        return Err(VerifierError::EnvironmentError(format!("Language risk analysis failed: {}", e)));
                    }
                }
            }
        }

        // ── 共享状态 ──
        if self.dimensions.shared_state {
            match self.client.get_shared_state_access(snapshot_id, &entity_keys).await {
                Ok(resp) => {
                    for access in &resp.accesses {
                        findings.push(FindingCandidate {
                            finding_id: format!("graph-risk-shared-{}", uuid::Uuid::new_v4()),
                            target_id: unit.target_id.clone(),
                            rule_id: "graph_risk.shared_state".into(),
                            verifier_id: "graph-risk".into(),
                            severity: FindingSeverity::High,
                            category: FindingCategory::Bug,
                            message: format!(
                                "{} access to shared state '{}'",
                                access.access_kind,
                                access.variable.as_ref().map(|v| v.qualified_name.as_str()).unwrap_or("?")
                            ),
                            ..Default::default()
                        });
                    }
                }
                Err(e) => {
                    warn!("shared state analysis failed: {}", e);
                    if self.enforcement.is_blocking() {
                        return Err(VerifierError::EnvironmentError(format!("Shared state analysis failed: {}", e)));
                    }
                }
            }
        }

        // 其余维度（dependency_closure / affected_tests）同理，P7 MVP 简化

        info!(findings = findings.len(), "GraphRisk completed");
        Ok(findings)
    }
}
```

---

## 五、Step 4：集成到 VerifierScheduler

**文件**：`crates/onto-assurance-runtime/src/verification/scheduler.rs`（或新建 `graph_verifier_scheduler.rs`）

```rust
//! GraphVerifierScheduler — 将 GraphIntegrity / GraphRisk 接入 UnitScheduler

use async_trait::async_trait;
use onto_assurance_types::verification_plan::{VerificationUnit, VerifierKind};
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_ledger::{
    ScopeLedger, TargetDisposition,
    RuleCoverageLedger, RuleCoverageStatus,
    VerifierExecutionLedger, VerifierExecutionEntry, VerifierExecutionStatus,
};
use onto_graph_verifiers::{
    GraphIntegrityVerifier, GraphRiskVerifier,
    EnforcementLevel, classify_change, integrity_policy, risk_policy,
};
use std::sync::Arc;
use tracing::{info, warn};

pub struct GraphVerifierScheduler {
    integrity: Arc<GraphIntegrityVerifier>,
    risk: Arc<GraphRiskVerifier>,
    scope_ledger: Arc<std::sync::Mutex<ScopeLedger>>,
    rule_ledger: Arc<std::sync::Mutex<RuleCoverageLedger>>,
    execution_ledger: Arc<std::sync::Mutex<VerifierExecutionLedger>>,
}

impl GraphVerifierScheduler {
    pub fn new(
        integrity: Arc<GraphIntegrityVerifier>,
        risk: Arc<GraphRiskVerifier>,
        scope_ledger: Arc<std::sync::Mutex<ScopeLedger>>,
        rule_ledger: Arc<std::sync::Mutex<RuleCoverageLedger>>,
        execution_ledger: Arc<std::sync::Mutex<VerifierExecutionLedger>>,
    ) -> Self {
        Self { integrity, risk, scope_ledger, rule_ledger, execution_ledger }
    }
}

#[async_trait]
impl crate::verification::scheduler::UnitScheduler for GraphVerifierScheduler {
    async fn schedule(&self, unit: &VerificationUnit)
        -> crate::verification::scheduler::ScheduleResult
    {
        let target_id = &unit.target_id;
        let unit_id = &unit.unit_id;

        // 1. 提取 changed_files 并分类
        let changed_files: Vec<String> = unit.metadata
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let ct = classify_change(&changed_files);
        let i_policy = integrity_policy(ct);
        let r_policy = risk_policy(ct);

        info!(%target_id, ?ct, ?i_policy, ?r_policy, "GraphVerifierScheduler dispatch");

        let mut all_findings = Vec::new();

        // 2. Graph Integrity
        if i_policy.should_invoke() {
            let start = chrono::Utc::now().to_rfc3339();
            match self.integrity.verify(unit).await {
                Ok(f) => {
                    all_findings.extend(f);
                    // 记录成功
                    self.scope_ledger.lock().unwrap().record(
                        target_id.clone(), TargetDisposition::Verified);
                    self.execution_ledger.lock().unwrap().record(
                        unit_id.clone(), "graph-integrity".into(),
                        VerifierExecutionStatus::Completed, None, Some(start), Some(chrono::Utc::now().to_rfc3339()));
                    self.rule_ledger.lock().unwrap().record(
                        format!("{}::graph_integrity", target_id),
                        if i_policy.is_blocking() { RuleCoverageStatus::RequiredRuleExecuted }
                        else { RuleCoverageStatus::AdvisoryRuleExecuted });
                }
                Err(e) => {
                    warn!("GraphIntegrity failed: {}", e);
                    // 记录失败
                    let disposition = match &e {
                        crate::verification::ports::VerifierError::EnvironmentError(_) => TargetDisposition::EnvironmentFailed,
                        _ => TargetDisposition::ReadFailed,
                    };
                    self.scope_ledger.lock().unwrap().record(target_id.clone(), disposition);
                    self.execution_ledger.lock().unwrap().record(
                        unit_id.clone(), "graph-integrity".into(),
                        VerifierExecutionStatus::Unavailable, None, Some(start), Some(chrono::Utc::now().to_rfc3339()));
                    self.rule_ledger.lock().unwrap().record(
                        format!("{}::graph_integrity", target_id),
                        RuleCoverageStatus::RuleExecutionFailed);

                    if i_policy.is_blocking() {
                        return Err(crate::verification::scheduler::ScheduleError::Internal(
                            format!("GraphIntegrity blocked: {}", e)));
                    }
                }
            }
        }

        // 3. Graph Risk
        if r_policy.should_invoke() {
            let start = chrono::Utc::now().to_rfc3339();
            match self.risk.verify(unit).await {
                Ok(f) => {
                    all_findings.extend(f);
                    self.scope_ledger.lock().unwrap().record(
                        target_id.clone(), TargetDisposition::Verified);
                    self.execution_ledger.lock().unwrap().record(
                        unit_id.clone(), "graph-risk".into(),
                        VerifierExecutionStatus::Completed, None, Some(start), Some(chrono::Utc::now().to_rfc3339()));
                    self.rule_ledger.lock().unwrap().record(
                        format!("{}::graph_risk", target_id),
                        if r_policy.is_blocking() { RuleCoverageStatus::RequiredRuleExecuted }
                        else { RuleCoverageStatus::AdvisoryRuleExecuted });
                }
                Err(e) => {
                    warn!("GraphRisk failed: {}", e);
                    self.execution_ledger.lock().unwrap().record(
                        unit_id.clone(), "graph-risk".into(),
                        VerifierExecutionStatus::Unavailable, None, Some(start), Some(chrono::Utc::now().to_rfc3339()));
                    if r_policy.is_blocking() {
                        self.scope_ledger.lock().unwrap().record(target_id.clone(), TargetDisposition::EnvironmentFailed);
                        self.rule_ledger.lock().unwrap().record(
                            format!("{}::graph_risk", target_id),
                            RuleCoverageStatus::RuleExecutionFailed);
                        return Err(crate::verification::scheduler::ScheduleError::Internal(
                            format!("GraphRisk blocked: {}", e)));
                    }
                }
            }
        }

        Ok(all_findings)
    }
}
```

---

## 六、Step 5：Workspace 集成

### 6.1 `OntoOS/Cargo.toml`

```toml
[workspace]
members = [
    # ... existing 10 ...
    "crates/onto-graph-verifiers",
]

[workspace.dependencies]
onto-graph-verifiers = { path = "crates/onto-graph-verifiers" }
```

### 6.2 在 `onto-assurance-runtime/Cargo.toml` 中可选依赖

```toml
[dependencies]
onto-graph-verifiers = { path = "../onto-graph-verifiers", optional = true }
# ... 或直接作为必须依赖
```

---

## 七、测试计划

### 单元测试

| # | 测试 | 文件 |
|---|------|------|
| 1 | SEALED snapshot → verify() returns Ok(vec![]) | graph_integrity.rs |
| 2 | NOT SEALED → StaleSnapshot error | graph_integrity.rs |
| 3 | checkpoint_mismatch → StaleSnapshot error | graph_integrity.rs |
| 4 | generation_mismatch → StaleSnapshot error | graph_integrity.rs |
| 5 | unprocessed_files → VerificationIncomplete (with Findings) | graph_integrity.rs |
| 6 | missing_passes → VerificationIncomplete (with Findings) | graph_integrity.rs |
| 7 | gRPC ConnectionFailed → EnvironmentError | graph_integrity.rs |
| 8 | gRPC GrpcError → EnvironmentError | graph_integrity.rs |
| 9 | High fan-out callers → High severity Finding | graph_risk.rs |
| 10 | unsafe block → Critical severity Finding | graph_risk.rs |
| 11 | Advisory mode + gRPC fail → Ok (warning, not error) | graph_risk.rs |
| 12 | Required mode + gRPC fail → EnvironmentError | graph_risk.rs |
| 13–18 | All 6 schedule_rules ChangeType → correct policy | schedule_rules.rs |
| 19 | Mixed file types → highest ChangeType chosen | schedule_rules.rs |
| 20 | Ledger writes on Integrity fail | scheduler tests |

### 集成测试

| # | 测试 |
|---|------|
| 21 | 真实 ofg-service + SEALED snapshot → Integrity passes |
| 22 | BUILDING snapshot → StaleSnapshot |
| 23 | 不存在的 repository → StaleSnapshot |
| 24 | 真实边数据 + CTE → call_chain 返回正确结果 |

---

## 八、执行顺序（强依赖）

```
Step 1: VerifierError 加 4 变体             (1 file, 5 min)
   ↓
Step 2: ofg-service proto + 实现             (6 files, ~500 lines)
   ├── 2a. snapshot_query.proto
   ├── 2b. graph_risk.proto
   ├── 2c. snapshot_query_service.rs
   ├── 2d. graph_risk_service.rs
   ├── 2e. build.rs 修改
   └── 2f. main.rs 注册
   ↓
Step 3: onto-graph-verifiers crate           (6 files, ~500 lines)
   ├── 3a. Cargo.toml
   ├── 3b. client.rs
   ├── 3c. schedule_rules.rs + tests
   ├── 3d. graph_integrity.rs + tests
   ├── 3e. graph_risk.rs + tests
   └── 3f. lib.rs
   ↓
Step 4: VerifierScheduler + 三本总账         (1 file, ~150 lines)
   ↓
Step 5: cargo build --workspace && cargo test --workspace
```

---

## 九、P7 退出标准

- [ ] GraphIntegrity 对所有代码变更强制执行
- [ ] VerifierScheduler 确定性调用（不依赖 Agent 决策）
- [ ] STALE / PARTIAL / UNSUPPORTED 不能进入 Positive Evidence
- [ ] 每个 gap 显式记录在 ScopeLedger / RuleCoverageLedger / VerifierExecutionLedger
- [ ] `cargo build --workspace` 通过
- [ ] `cargo test --workspace` 全部通过（含 20+ 个新增测试）

---

## 十、不在 P7 范围

- ❌ Runtime Agent 可选图谱查询（P9）
- ❌ 权威 Outbox / Redis Streams / Projector（P8）
- ❌ SemanticReviewVerifier（P10）
- ❌ Agent 调用 Graph Verifier
